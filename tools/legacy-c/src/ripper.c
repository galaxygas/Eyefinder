/*
 * GTA III Music Ripper
 *
 * Extracts every streamed audio file of the PC version of Grand Theft Auto III
 * (radio stations, police radio / ambience, announcements, cutscene audio,
 * mission-complete / game-complete music, mission audio), the audio tracks of
 * the intro movies, and the whole SFX.RAW sample bank, to standard WAV files
 * in "<game folder>\Extracted Audio".
 *
 * Built as an .asi plugin it runs on a background thread when the game starts.
 * Built as a console program it takes the game folder as its first argument.
 *
 * Source formats are detected from the file contents, not the extension:
 *   RIFF/WAVE PCM (any bit depth), IEEE float, WAVE_FORMAT_EXTENSIBLE  -> copied bit-exact
 *   IMA ADPCM (0x0011), Xbox ADPCM (0x0069), MS ADPCM (0x0002)          -> decoded to 16-bit PCM
 *   MPEG audio layer I/II/III (raw .mp3 or inside RIFF 0x0050/0x0055)  -> decoded (float32 by default)
 *   MPEG-1/2 program streams (.mpg)                                     -> audio demuxed + decoded
 *   SFX.SDT / SFX.RAW                                                   -> 16-bit mono PCM per entry
 */

#define _CRT_SECURE_NO_WARNINGS
#define _FILE_OFFSET_BITS 64

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdarg.h>
#include <time.h>
#include <ctype.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#else
#include <sys/stat.h>
#include <dirent.h>
#include <unistd.h>
#endif

#define MINIMP3_IMPLEMENTATION
#define MINIMP3_FLOAT_OUTPUT
#include "minimp3.h"

#include "stream_table.h"
#include "sfx_names.h"

#define RIPPER_VERSION "1.0.0"
#define PATHMAX 1024

/* ------------------------------------------------------------------------ */
/* Configuration                                                             */
/* ------------------------------------------------------------------------ */

typedef struct {
	int enabled;
	int force;
	int mpegFloat;      /* 1: MPEG audio decoded to 32-bit float, 0: 16-bit PCM */
	int sfx;
	int missionAudio;
	int movies;
	int otherFiles;
	int keepMovieBitstream;
	char outFolder[260];
} Config;

static Config g_cfg = { 1, 0, 1, 1, 1, 1, 1, 1, "Extracted Audio" };

/* ------------------------------------------------------------------------ */
/* File system helpers (UTF-8 paths everywhere, wide APIs on Windows)       */
/* ------------------------------------------------------------------------ */

#ifdef _WIN32
#define SEP '\\'
#define SEPS "\\"
static wchar_t *widen(const char *s)
{
	int n = MultiByteToWideChar(CP_UTF8, 0, s, -1, NULL, 0);
	wchar_t *w = (wchar_t *)malloc(sizeof(wchar_t) * (n > 0 ? n : 1));
	if (n <= 0) { w[0] = 0; return w; }
	MultiByteToWideChar(CP_UTF8, 0, s, -1, w, n);
	return w;
}
static void narrow(const wchar_t *w, char *out, int outSize)
{
	if (WideCharToMultiByte(CP_UTF8, 0, w, -1, out, outSize, NULL, NULL) <= 0) out[0] = 0;
}
static FILE *xfopen(const char *path, const char *mode)
{
	wchar_t *wp = widen(path), *wm = widen(mode);
	FILE *f = _wfopen(wp, wm);
	free(wp); free(wm);
	return f;
}
static int xmkdir(const char *path)
{
	wchar_t *wp = widen(path);
	BOOL ok = CreateDirectoryW(wp, NULL) || GetLastError() == ERROR_ALREADY_EXISTS;
	free(wp);
	return ok ? 0 : -1;
}
static int xexists(const char *path)
{
	wchar_t *wp = widen(path);
	DWORD a = GetFileAttributesW(wp);
	free(wp);
	return a != INVALID_FILE_ATTRIBUTES;
}
static int xrename(const char *from, const char *to)
{
	wchar_t *a = widen(from), *b = widen(to);
	BOOL ok = MoveFileExW(a, b, MOVEFILE_REPLACE_EXISTING);
	free(a); free(b);
	return ok ? 0 : -1;
}
static void xremove(const char *path)
{
	wchar_t *wp = widen(path);
	DeleteFileW(wp);
	free(wp);
}
#define xfseek(f, o, w) _fseeki64(f, o, w)
#define xftell(f) _ftelli64(f)
#else
#define SEP '/'
#define SEPS "/"
static FILE *xfopen(const char *path, const char *mode) { return fopen(path, mode); }
static int xmkdir(const char *path) { return (mkdir(path, 0755) == 0 || access(path, F_OK) == 0) ? 0 : -1; }
static int xexists(const char *path) { return access(path, F_OK) == 0; }
static int xrename(const char *from, const char *to) { return rename(from, to); }
static void xremove(const char *path) { remove(path); }
#define xfseek(f, o, w) fseeko(f, o, w)
#define xftell(f) ftello(f)
#endif

typedef struct { char **names; int count; } DirList;

static void dirlist_free(DirList *d)
{
	int i;
	for (i = 0; i < d->count; i++) free(d->names[i]);
	free(d->names);
	d->names = NULL; d->count = 0;
}

static void dirlist_add(DirList *d, const char *name)
{
	d->names = (char **)realloc(d->names, sizeof(char *) * (d->count + 1));
	d->names[d->count++] = strdup(name);
}

static int cmp_ci(const char *a, const char *b)
{
	for (;; a++, b++) {
		int ca = tolower((unsigned char)*a), cb = tolower((unsigned char)*b);
		if (ca != cb || !ca) return ca - cb;
	}
}

static int qsort_ci(const void *a, const void *b) { return cmp_ci(*(const char *const *)a, *(const char *const *)b); }

/* Lists regular files in a directory, sorted case-insensitively. */
static void list_files(const char *dir, DirList *out)
{
	out->names = NULL; out->count = 0;
#ifdef _WIN32
	{
		char pat[PATHMAX];
		wchar_t *wp;
		WIN32_FIND_DATAW fd;
		HANDLE h;
		snprintf(pat, sizeof(pat), "%s\\*", dir);
		wp = widen(pat);
		h = FindFirstFileW(wp, &fd);
		free(wp);
		if (h == INVALID_HANDLE_VALUE) return;
		do {
			char name[PATHMAX];
			if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) continue;
			narrow(fd.cFileName, name, sizeof(name));
			dirlist_add(out, name);
		} while (FindNextFileW(h, &fd));
		FindClose(h);
	}
#else
	{
		DIR *d = opendir(dir);
		struct dirent *e;
		if (!d) return;
		while ((e = readdir(d))) {
			char full[PATHMAX];
			struct stat st;
			snprintf(full, sizeof(full), "%s/%s", dir, e->d_name);
			if (stat(full, &st) == 0 && S_ISREG(st.st_mode)) dirlist_add(out, e->d_name);
		}
		closedir(d);
	}
#endif
	if (out->count > 1) qsort(out->names, out->count, sizeof(char *), qsort_ci);
}

/* Finds a file in a directory ignoring case (the game ships mixed-case names). */
static int find_file_ci(const char *dir, const char *name, char *outPath, size_t outSize)
{
	DirList dl;
	int i, found = 0;
	list_files(dir, &dl);
	for (i = 0; i < dl.count; i++) {
		if (cmp_ci(dl.names[i], name) == 0) {
			snprintf(outPath, outSize, "%s%c%s", dir, SEP, dl.names[i]);
			found = 1;
			break;
		}
	}
	dirlist_free(&dl);
	return found;
}

static int64_t file_size(const char *path)
{
	FILE *f = xfopen(path, "rb");
	int64_t s;
	if (!f) return -1;
	xfseek(f, 0, SEEK_END);
	s = xftell(f);
	fclose(f);
	return s;
}

/* Replaces characters Windows does not allow in file names. */
static void sanitize(char *s)
{
	for (; *s; s++)
		if (strchr("<>:\"/\\|?*", *s) || (unsigned char)*s < 32) *s = '_';
}

/* ------------------------------------------------------------------------ */
/* Logging                                                                   */
/* ------------------------------------------------------------------------ */

static FILE *g_log;

static void logf_(const char *fmt, ...)
{
	va_list ap;
	if (g_log) {
		va_start(ap, fmt);
		vfprintf(g_log, fmt, ap);
		va_end(ap);
		fputc('\n', g_log);
		fflush(g_log);
	}
#ifndef RIPPER_ASI
	va_start(ap, fmt);
	vprintf(fmt, ap);
	va_end(ap);
	putchar('\n');
#endif
}

/* ------------------------------------------------------------------------ */
/* Small byte helpers                                                        */
/* ------------------------------------------------------------------------ */

static uint16_t rd16(const uint8_t *p) { return (uint16_t)(p[0] | (p[1] << 8)); }
static uint32_t rd32(const uint8_t *p) { return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24); }
static void wr16(uint8_t *p, uint16_t v) { p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); }
static void wr32(uint8_t *p, uint32_t v) { p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); p[2] = (uint8_t)(v >> 16); p[3] = (uint8_t)(v >> 24); }

typedef struct { uint8_t *p; size_t len, cap; } Buf;

static void buf_put(Buf *b, const void *data, size_t n)
{
	if (b->len + n > b->cap) {
		b->cap = (b->len + n) * 2 + 64;
		b->p = (uint8_t *)realloc(b->p, b->cap);
	}
	memcpy(b->p + b->len, data, n);
	b->len += n;
}
static void buf_put32(Buf *b, uint32_t v) { uint8_t t[4]; wr32(t, v); buf_put(b, t, 4); }
static void buf_free(Buf *b) { free(b->p); b->p = NULL; b->len = b->cap = 0; }

/* Appends a RIFF sub-chunk with even padding. */
static void buf_chunk(Buf *b, const char *id, const void *data, uint32_t n)
{
	buf_put(b, id, 4);
	buf_put32(b, n);
	buf_put(b, data, n);
	if (n & 1) buf_put(b, "", 1);
}

static void fmt_time(double sec, char *out, size_t n)
{
	long long ms = (long long)(sec * 1000.0 + 0.5);
	snprintf(out, n, "%02lld:%02lld:%02lld.%03lld", ms / 3600000, (ms / 60000) % 60, (ms / 1000) % 60, ms % 1000);
}

/* ------------------------------------------------------------------------ */
/* JSON / CSV manifest                                                       */
/* ------------------------------------------------------------------------ */

static FILE *g_json, *g_csv, *g_txt;
static int g_jsonFirst = 1;
static int g_countOk, g_countFail;

static void json_str(FILE *f, const char *s)
{
	fputc('"', f);
	for (; s && *s; s++) {
		unsigned char c = (unsigned char)*s;
		if (c == '"' || c == '\\') fprintf(f, "\\%c", c);
		else if (c < 32) fprintf(f, "\\u%04x", c);
		else fputc(c, f);
	}
	fputc('"', f);
}

static void csv_str(FILE *f, const char *s)
{
	fputc('"', f);
	for (; s && *s; s++) {
		if (*s == '"') fputc('"', f);
		fputc(*s, f);
	}
	fputc('"', f);
}

/* ------------------------------------------------------------------------ */
/* WAV writer                                                                */
/* ------------------------------------------------------------------------ */

typedef struct {
	uint16_t tag;        /* 1 = PCM, 3 = IEEE float, 0xFFFE = extensible */
	uint16_t channels;
	uint32_t rate;
	uint16_t bits;
	uint16_t blockAlign;
	uint8_t raw[64];     /* verbatim fmt chunk when copying PCM / float / extensible */
	uint32_t rawLen;
} OutFormat;

typedef struct {
	FILE *f;
	char tmpPath[PATHMAX], path[PATHMAX];
	OutFormat fmt;
	uint64_t dataBytes;
	int64_t factPos, dataSizePos;
	int failed;
} WavOut;

static int wav_open(WavOut *w, const char *path, const OutFormat *fmt)
{
	uint8_t h[64];
	uint32_t fmtLen;
	memset(w, 0, sizeof(*w));
	w->fmt = *fmt;
	snprintf(w->path, sizeof(w->path), "%s", path);
	snprintf(w->tmpPath, sizeof(w->tmpPath), "%s.partial", path);
	w->f = xfopen(w->tmpPath, "wb");
	if (!w->f) { logf_("  ERROR: cannot create %s", w->tmpPath); return -1; }
	setvbuf(w->f, NULL, _IOFBF, 1 << 20);

	fwrite("RIFF\0\0\0\0WAVE", 1, 12, w->f);
	if (fmt->rawLen) {
		fwrite("fmt ", 1, 4, w->f);
		wr32(h, fmt->rawLen); fwrite(h, 1, 4, w->f);
		fwrite(fmt->raw, 1, fmt->rawLen, w->f);
		if (fmt->rawLen & 1) fputc(0, w->f);
	} else {
		fmtLen = fmt->tag == 3 ? 18 : 16;
		memcpy(h, "fmt ", 4); wr32(h + 4, fmtLen);
		wr16(h + 8, fmt->tag); wr16(h + 10, fmt->channels); wr32(h + 12, fmt->rate);
		wr32(h + 16, fmt->rate * fmt->blockAlign); wr16(h + 20, fmt->blockAlign); wr16(h + 22, fmt->bits);
		wr16(h + 24, 0);
		fwrite(h, 1, 8 + fmtLen, w->f);
	}
	w->factPos = -1;
	if (fmt->tag != 1) {
		/* non-PCM WAV files must carry a fact chunk with the frame count */
		fwrite("fact\4\0\0\0", 1, 8, w->f);
		w->factPos = xftell(w->f);
		fwrite("\0\0\0\0", 1, 4, w->f);
	}
	fwrite("data", 1, 4, w->f);
	w->dataSizePos = xftell(w->f);
	fwrite("\0\0\0\0", 1, 4, w->f);
	return 0;
}

static void wav_write(WavOut *w, const void *data, size_t bytes)
{
	if (!w->f || w->failed) return;
	if (w->dataBytes + bytes > 0xFFFFFF00u) { w->failed = 1; logf_("  ERROR: output exceeds the 4 GB WAV limit"); return; }
	if (fwrite(data, 1, bytes, w->f) != bytes) { w->failed = 1; logf_("  ERROR: write failed (disk full?) %s", w->tmpPath); return; }
	w->dataBytes += bytes;
}

/* Finishes the file. extra = additional chunks (cue, LIST) appended after data. */
static int wav_close(WavOut *w, const Buf *extra)
{
	uint8_t t[4];
	int64_t end;
	if (!w->f) return -1;
	if (w->dataBytes & 1) fputc(0, w->f);
	if (extra && extra->len) fwrite(extra->p, 1, extra->len, w->f);
	end = xftell(w->f);
	xfseek(w->f, 4, SEEK_SET); wr32(t, (uint32_t)(end - 8)); fwrite(t, 1, 4, w->f);
	xfseek(w->f, w->dataSizePos, SEEK_SET); wr32(t, (uint32_t)w->dataBytes); fwrite(t, 1, 4, w->f);
	if (w->factPos >= 0) {
		xfseek(w->f, w->factPos, SEEK_SET);
		wr32(t, (uint32_t)(w->dataBytes / (w->fmt.blockAlign ? w->fmt.blockAlign : 1)));
		fwrite(t, 1, 4, w->f);
	}
	if (fflush(w->f) != 0) w->failed = 1;
	fclose(w->f);
	w->f = NULL;
	if (w->failed) { xremove(w->tmpPath); return -1; }
	xremove(w->path);
	if (xrename(w->tmpPath, w->path) != 0) { logf_("  ERROR: cannot rename %s", w->tmpPath); return -1; }
	return 0;
}

static void wav_abort(WavOut *w)
{
	if (w->f) { fclose(w->f); w->f = NULL; xremove(w->tmpPath); }
}

/* LIST/INFO chunk: title, product, comment, software. */
static void build_info(Buf *b, const char *title, const char *comment)
{
	Buf l = { 0 };
	const char *prd = "Grand Theft Auto III (PC)";
	const char *sft = "GTA III Music Ripper " RIPPER_VERSION;
	buf_put(&l, "INFO", 4);
	if (title) buf_chunk(&l, "INAM", title, (uint32_t)strlen(title) + 1);
	buf_chunk(&l, "IPRD", prd, (uint32_t)strlen(prd) + 1);
	if (comment) buf_chunk(&l, "ICMT", comment, (uint32_t)strlen(comment) + 1);
	buf_chunk(&l, "ISFT", sft, (uint32_t)strlen(sft) + 1);
	buf_chunk(b, "LIST", l.p, (uint32_t)l.len);
	buf_free(&l);
}

/* ------------------------------------------------------------------------ */
/* Markers read from the source WAV (cue / LIST adtl / smpl)                */
/* ------------------------------------------------------------------------ */

typedef struct {
	uint32_t id;
	uint32_t frame;      /* dwSampleOffset from the cue chunk */
	uint32_t length;     /* ltxt sample length, 0 if none */
	char label[256];
	char note[256];
} Marker;

typedef struct {
	Marker *m;
	int count;
	uint8_t *cueRaw; uint32_t cueLen;     /* verbatim chunks, re-embedded in the full output */
	uint8_t *adtlRaw; uint32_t adtlLen;
	int hasSmpl;
	uint32_t smplLoops;
	uint32_t loopStart[8], loopEnd[8];
} MarkerSet;

static void markers_free(MarkerSet *s)
{
	free(s->m); free(s->cueRaw); free(s->adtlRaw);
	memset(s, 0, sizeof(*s));
}

static Marker *marker_by_id(MarkerSet *s, uint32_t id)
{
	int i;
	for (i = 0; i < s->count; i++) if (s->m[i].id == id) return &s->m[i];
	return NULL;
}

static void copy_text(char *dst, size_t dstSize, const uint8_t *src, uint32_t n)
{
	size_t i, k = 0;
	for (i = 0; i < n && src[i] && k + 1 < dstSize; i++) dst[k++] = (char)src[i];
	dst[k] = 0;
}

static void parse_cue(MarkerSet *s, const uint8_t *p, uint32_t n)
{
	uint32_t count, i;
	if (n < 4) return;
	count = rd32(p);
	if (count > (n - 4) / 24) count = (n - 4) / 24;
	s->m = (Marker *)calloc(count ? count : 1, sizeof(Marker));
	for (i = 0; i < count; i++) {
		const uint8_t *e = p + 4 + i * 24;
		s->m[i].id = rd32(e);
		s->m[i].frame = rd32(e + 20);
	}
	s->count = (int)count;
	s->cueRaw = (uint8_t *)malloc(n); memcpy(s->cueRaw, p, n); s->cueLen = n;
}

static void parse_adtl(MarkerSet *s, const uint8_t *p, uint32_t n)
{
	uint32_t pos = 4;
	while (pos + 8 <= n) {
		uint32_t len = rd32(p + pos + 4);
		const uint8_t *d = p + pos + 8;
		Marker *m;
		if (len > n - pos - 8) break;
		if (len >= 4 && (m = marker_by_id(s, rd32(d)))) {
			if (!memcmp(p + pos, "labl", 4)) copy_text(m->label, sizeof(m->label), d + 4, len - 4);
			else if (!memcmp(p + pos, "note", 4)) copy_text(m->note, sizeof(m->note), d + 4, len - 4);
			else if (!memcmp(p + pos, "ltxt", 4) && len >= 8) {
				m->length = rd32(d + 4);
				if (len > 20 && !m->note[0]) copy_text(m->note, sizeof(m->note), d + 20, len - 20);
			}
		}
		pos += 8 + len + (len & 1);
	}
}

static void parse_smpl(MarkerSet *s, const uint8_t *p, uint32_t n)
{
	uint32_t i, loops;
	if (n < 36) return;
	s->hasSmpl = 1;
	loops = rd32(p + 28);
	if (loops > (n - 36) / 24) loops = (n - 36) / 24;
	s->smplLoops = loops;
	for (i = 0; i < loops && i < 8; i++) {
		s->loopStart[i] = rd32(p + 36 + i * 24 + 8);
		s->loopEnd[i] = rd32(p + 36 + i * 24 + 12);
	}
}

static int cmp_marker(const void *a, const void *b)
{
	const Marker *x = (const Marker *)a, *y = (const Marker *)b;
	return x->frame < y->frame ? -1 : x->frame > y->frame;
}

/* ------------------------------------------------------------------------ */
/* Output sink: one full-length file plus optional per-marker track files   */
/* ------------------------------------------------------------------------ */

typedef struct {
	char path[PATHMAX];
	char title[256];
	const char *comment;
	OutFormat fmt;
	int opened, failed;
	WavOut full;
	uint64_t frames;
	Buf extra;           /* chunks to append to the full file (cue, adtl) */

	/* track split */
	MarkerSet *markers;  /* sorted by frame */
	char trackDir[PATHMAX];
	int trackIdx;        /* index of the segment currently written, -1 none */
	WavOut track;
	int tracksWritten;
} Sink;

static uint64_t seg_end(Sink *s, int i) { return i < s->markers->count ? s->markers->m[i].frame : UINT64_MAX; }

static void segment_name(Sink *s, int i, char *out, size_t n)
{
	char label[300];
	if (i == 0) snprintf(label, sizeof(label), "before first marker");
	else {
		Marker *m = &s->markers->m[i - 1];
		if (m->label[0]) snprintf(label, sizeof(label), "%s", m->label);
		else snprintf(label, sizeof(label), "marker %u", m->id);
	}
	sanitize(label);
	snprintf(out, n, "%s%c%03d - %s.wav", s->trackDir, SEP, i, label);
}

static int sink_init(Sink *s, const char *path, const char *title, const char *comment, MarkerSet *markers, const char *trackDir)
{
	memset(s, 0, sizeof(*s));
	snprintf(s->path, sizeof(s->path), "%s", path);
	snprintf(s->title, sizeof(s->title), "%s", title);
	s->comment = comment;
	s->trackIdx = -1;
	if (markers && markers->count > 0 && trackDir) {
		qsort(markers->m, markers->count, sizeof(Marker), cmp_marker);
		s->markers = markers;
		snprintf(s->trackDir, sizeof(s->trackDir), "%s", trackDir);
	}
	if (markers) {
		if (markers->cueLen) buf_chunk(&s->extra, "cue ", markers->cueRaw, markers->cueLen);
		if (markers->adtlLen) buf_chunk(&s->extra, "LIST", markers->adtlRaw, markers->adtlLen);
	}
	return 0;
}

static void sink_close_track(Sink *s)
{
	if (s->trackIdx >= 0 && s->track.f) {
		Buf info = { 0 };
		char t[400];
		snprintf(t, sizeof(t), "%s - segment %d", s->title, s->trackIdx);
		build_info(&info, t, s->comment);
		if (wav_close(&s->track, &info) == 0) s->tracksWritten++;
		buf_free(&info);
	}
	s->trackIdx = -1;
}

/* Writes interleaved frames in the sink's format. */
static void sink_write(Sink *s, const OutFormat *fmt, const uint8_t *data, uint64_t frames)
{
	if (s->failed || !frames) return;
	if (!s->opened) {
		s->fmt = *fmt;
		if (wav_open(&s->full, s->path, fmt) != 0) { s->failed = 1; return; }
		s->opened = 1;
		if (s->markers) xmkdir(s->trackDir);
	}
	wav_write(&s->full, data, (size_t)(frames * fmt->blockAlign));
	if (s->full.failed) { s->failed = 1; return; }

	while (s->markers && frames) {
		int seg = 0;
		uint64_t n;
		while (seg < s->markers->count && s->frames >= seg_end(s, seg)) seg++;
		if (seg != s->trackIdx) {
			char p[PATHMAX];
			sink_close_track(s);
			/* skip a zero-length leading segment when the first marker is at 0 */
			segment_name(s, seg, p, sizeof(p));
			if (wav_open(&s->track, p, fmt) != 0) { s->markers = NULL; break; }
			s->trackIdx = seg;
		}
		n = seg_end(s, seg) - s->frames;
		if (n > frames) n = frames;
		wav_write(&s->track, data, (size_t)(n * fmt->blockAlign));
		data += n * fmt->blockAlign;
		frames -= n;
		s->frames += n;
	}
	if (!s->markers) s->frames += frames;
}

static int sink_finish(Sink *s)
{
	Buf info = { 0 };
	int r;
	sink_close_track(s);
	if (!s->opened) { buf_free(&s->extra); return -1; }
	if (s->failed) { wav_abort(&s->full); buf_free(&s->extra); return -1; }
	buf_put(&info, s->extra.p ? s->extra.p : (const uint8_t *)"", s->extra.len);
	build_info(&info, s->title, s->comment);
	r = wav_close(&s->full, &info);
	buf_free(&info);
	buf_free(&s->extra);
	return r;
}

/* ------------------------------------------------------------------------ */
/* Source description recorded in the manifest                              */
/* ------------------------------------------------------------------------ */

typedef struct {
	char codec[64];
	uint32_t rate, channels, bits, blockAlign, bitrateKbps;
	int64_t size;
	uint32_t mpegLayer;
	int hasLame;
	uint32_t encDelay, encPadding;
	uint64_t mismatchedFrames;
	char extra[512];
} SourceInfo;

/* ------------------------------------------------------------------------ */
/* IMA ADPCM (Microsoft "Multimedia Standards Update" IMA/DVI layout)       */
/* ------------------------------------------------------------------------ */

static const int kImaStep[89] = {
	7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45,
	50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230,
	253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963,
	1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327,
	3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442,
	11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767
};
static const int kImaIndex[16] = { -1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8 };

static int16_t ima_nibble(int *pred, int *index, int n)
{
	int step = kImaStep[*index];
	int diff = step >> 3;
	if (n & 4) diff += step;
	if (n & 2) diff += step >> 1;
	if (n & 1) diff += step >> 2;
	if (n & 8) *pred -= diff; else *pred += diff;
	if (*pred > 32767) *pred = 32767; else if (*pred < -32768) *pred = -32768;
	*index += kImaIndex[n];
	if (*index < 0) *index = 0; else if (*index > 88) *index = 88;
	return (int16_t)*pred;
}

/* Decodes one block; returns frames produced. Handles a short final block. */
static uint32_t ima_block(const uint8_t *blk, uint32_t len, int ch, uint32_t spb, int16_t *out)
{
	int pred[8], idx[8], c;
	uint32_t frames, f, groups, g;
	if (len < (uint32_t)(4 * ch)) return 0;
	for (c = 0; c < ch; c++) {
		pred[c] = (int16_t)rd16(blk + c * 4);
		idx[c] = blk[c * 4 + 2];
		if (idx[c] > 88) idx[c] = 88;
		out[c] = (int16_t)pred[c];
	}
	/* each 4-byte group per channel holds 8 samples, low nibble first */
	groups = (len - 4 * ch) / (4 * ch);
	frames = 1 + groups * 8;
	if (frames > spb) { frames = spb; groups = (spb - 1) / 8; }
	for (g = 0; g < groups; g++) {
		for (c = 0; c < ch; c++) {
			const uint8_t *d = blk + 4 * ch + (g * ch + c) * 4;
			for (f = 0; f < 4; f++) {
				out[(1 + g * 8 + f * 2) * ch + c] = ima_nibble(&pred[c], &idx[c], d[f] & 15);
				out[(1 + g * 8 + f * 2 + 1) * ch + c] = ima_nibble(&pred[c], &idx[c], d[f] >> 4);
			}
		}
	}
	return frames;
}

/* ------------------------------------------------------------------------ */
/* Microsoft ADPCM                                                           */
/* ------------------------------------------------------------------------ */

static const int kMsAdapt[16] = { 230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230 };

static uint32_t msadpcm_block(const uint8_t *blk, uint32_t len, int ch, uint32_t spb, const int16_t (*coef)[2], int numCoef, int16_t *out)
{
	int c1[2], c2[2], delta[2], s1[2], s2[2], c;
	uint32_t frames, i, nib = 0, totalNibs;
	if (len < (uint32_t)(7 * ch)) return 0;
	for (c = 0; c < ch; c++) {
		int p = blk[c];
		if (p >= numCoef) p = numCoef - 1;
		c1[c] = coef[p][0]; c2[c] = coef[p][1];
		delta[c] = (int16_t)rd16(blk + ch + c * 2);
		s1[c] = (int16_t)rd16(blk + 3 * ch + c * 2);
		s2[c] = (int16_t)rd16(blk + 5 * ch + c * 2);
		out[c] = (int16_t)s2[c];
		out[ch + c] = (int16_t)s1[c];
	}
	totalNibs = (len - 7 * ch) * 2;
	frames = 2 + totalNibs / ch;
	if (frames > spb) frames = spb;
	for (i = 2; i < frames; i++) {
		for (c = 0; c < ch; c++, nib++) {
			uint8_t byte = blk[7 * ch + nib / 2];
			int n = (nib & 1) ? (byte & 15) : (byte >> 4);   /* high nibble first */
			int sn = n >= 8 ? n - 16 : n;
			int pred = (s1[c] * c1[c] + s2[c] * c2[c]) / 256;
			pred += sn * delta[c];
			if (pred > 32767) pred = 32767; else if (pred < -32768) pred = -32768;
			out[i * ch + c] = (int16_t)pred;
			s2[c] = s1[c]; s1[c] = pred;
			delta[c] = (kMsAdapt[n] * delta[c]) / 256;
			if (delta[c] < 16) delta[c] = 16;
		}
	}
	return frames;
}

/* ------------------------------------------------------------------------ */
/* MPEG audio (layers I/II/III) streaming decode                             */
/* ------------------------------------------------------------------------ */

static void float_to_s16(const float *in, int16_t *out, size_t n)
{
	size_t i;
	for (i = 0; i < n; i++) {
		float v = in[i] * 32768.0f;
		int s;
		v += v >= 0 ? 0.5f : -0.5f;
		s = (int)v;
		if (s > 32767) s = 32767; else if (s < -32768) s = -32768;
		out[i] = (int16_t)s;
	}
}

static uint32_t id3v2_size(const uint8_t *p, size_t n)
{
	if (n >= 10 && !memcmp(p, "ID3", 3) && !((p[6] | p[7] | p[8] | p[9]) & 0x80))
		return 10 + (((uint32_t)p[6] << 21) | ((uint32_t)p[7] << 14) | ((uint32_t)p[8] << 7) | p[9]) + ((p[5] & 0x10) ? 10 : 0);
	return 0;
}

/* Detects a Xing/Info header frame (not audio) and reads LAME delay/padding. */
static int xing_check(const uint8_t *frame, int frameBytes, SourceInfo *si)
{
	int i;
	for (i = 4; i + 8 <= frameBytes && i < 64; i++) {
		if (!memcmp(frame + i, "Xing", 4) || !memcmp(frame + i, "Info", 4)) {
			uint32_t flags = ((uint32_t)frame[i + 4] << 24) | ((uint32_t)frame[i + 5] << 16) | ((uint32_t)frame[i + 6] << 8) | frame[i + 7];
			int p = i + 8;
			if (flags & 1) p += 4;
			if (flags & 2) p += 4;
			if (flags & 4) p += 100;
			if (flags & 8) p += 4;
			if (p + 24 <= frameBytes && (!memcmp(frame + p, "LAME", 4) || !memcmp(frame + p, "Lavc", 4) || !memcmp(frame + p, "Lavf", 4))) {
				const uint8_t *d = frame + p + 21;
				si->hasLame = 1;
				si->encDelay = ((uint32_t)d[0] << 4) | (d[1] >> 4);
				si->encPadding = ((uint32_t)(d[1] & 15) << 8) | d[2];
			}
			return 1;
		}
	}
	return 0;
}

/* Decodes MPEG audio from file range [offset, offset+length). */
static int decode_mpeg(FILE *f, int64_t offset, int64_t length, Sink *sink, SourceInfo *si)
{
	enum { BUFSZ = 1 << 18, MARGIN = 1 << 14 };
	static mp3dec_t dec;
	uint8_t *buf = (uint8_t *)malloc(BUFSZ);
	float pcmf[MINIMP3_MAX_SAMPLES_PER_FRAME];
	int16_t pcm16[MINIMP3_MAX_SAMPLES_PER_FRAME];
	size_t avail = 0, pos = 0;
	int64_t left = length;
	int first = 1, eof = 0, locked = 0;
	OutFormat of;
	uint64_t bitSum = 0, frameCount = 0;

	memset(&of, 0, sizeof(of));
	mp3dec_init(&dec);
	xfseek(f, offset, SEEK_SET);
	for (;;) {
		mp3dec_frame_info_t fi;
		int samples;
		if (!eof && avail - pos < MARGIN) {
			size_t want, got;
			memmove(buf, buf + pos, avail - pos);
			avail -= pos; pos = 0;
			want = BUFSZ - avail;
			if ((int64_t)want > left) want = (size_t)left;
			got = fread(buf + avail, 1, want, f);
			avail += got; left -= got;
			if (got < want || left <= 0) eof = 1;
		}
		if (pos >= avail) break;
		if (first) {
			uint32_t skip = id3v2_size(buf + pos, avail - pos);
			if (skip) {
				if (skip <= avail - pos) { pos += skip; continue; }
				/* tag larger than buffer: skip in the file */
				skip -= (uint32_t)(avail - pos);
				xfseek(f, skip, SEEK_CUR); left -= skip; avail = pos = 0;
				continue;
			}
		}
		samples = mp3dec_decode_frame(&dec, buf + pos, (int)(avail - pos), pcmf, &fi);
		if (fi.frame_bytes == 0) {
			if (eof) break;
			pos = avail;   /* no sync in this window: discard it */
			continue;
		}
		if (first && samples > 0 && fi.layer == 3) {
			first = 0;
			if (xing_check(buf + pos + fi.frame_offset, fi.frame_bytes - fi.frame_offset, si)) { pos += fi.frame_bytes; continue; }
		}
		if (samples > 0) first = 0;
		pos += fi.frame_bytes;
		if (samples <= 0) continue;
		if (!locked) {
			locked = 1;
			of.channels = (uint16_t)fi.channels;
			of.rate = (uint32_t)fi.hz;
			if (g_cfg.mpegFloat) { of.tag = 3; of.bits = 32; }
			else { of.tag = 1; of.bits = 16; }
			of.blockAlign = (uint16_t)(of.channels * of.bits / 8);
			si->rate = of.rate; si->channels = of.channels; si->mpegLayer = (uint32_t)fi.layer;
			snprintf(si->codec, sizeof(si->codec), "MPEG-%s Layer %s", fi.hz < 32000 ? (fi.hz < 16000 ? "2.5" : "2") : "1",
				fi.layer == 1 ? "I" : fi.layer == 2 ? "II" : "III");
		}
		if ((uint32_t)fi.hz != of.rate || fi.channels != of.channels) { si->mismatchedFrames++; continue; }
		bitSum += (uint64_t)fi.bitrate_kbps; frameCount++;
		if (g_cfg.mpegFloat) sink_write(sink, &of, (const uint8_t *)pcmf, (uint64_t)samples);
		else {
			float_to_s16(pcmf, pcm16, (size_t)samples * of.channels);
			sink_write(sink, &of, (const uint8_t *)pcm16, (uint64_t)samples);
		}
		if (sink->failed) break;
	}
	if (frameCount) si->bitrateKbps = (uint32_t)(bitSum / frameCount);
	free(buf);
	return locked && !sink->failed ? 0 : -1;
}

static int looks_like_mpeg_audio(const uint8_t *p, size_t n)
{
	size_t i;
	if (n >= 3 && !memcmp(p, "ID3", 3)) return 1;
	for (i = 0; i + 1 < n && i < 4096; i++)
		if (p[i] == 0xFF && (p[i + 1] & 0xE0) == 0xE0 && ((p[i + 1] >> 1) & 3) != 0) return 1;
	return 0;
}

/* ------------------------------------------------------------------------ */
/* WAV / generic stream source                                               */
/* ------------------------------------------------------------------------ */

typedef struct {
	uint16_t tag, channels, blockAlign, bits, samplesPerBlock;
	uint32_t rate;
	uint8_t fmtRaw[64]; uint32_t fmtLen;
	int64_t dataOff, dataLen;
	int hasFact; uint32_t factFrames;
	int16_t coef[32][2]; int numCoef;
} WavIn;

static int parse_riff(FILE *f, int64_t fsize, WavIn *w, MarkerSet *ms)
{
	uint8_t h[12];
	int64_t pos = 12;
	int haveFmt = 0, haveData = 0;
	memset(w, 0, sizeof(*w));
	xfseek(f, 0, SEEK_SET);
	if (fread(h, 1, 12, f) != 12 || memcmp(h, "RIFF", 4) || memcmp(h + 8, "WAVE", 4)) return -1;
	while (pos + 8 <= fsize) {
		uint8_t ch[8];
		uint32_t len;
		xfseek(f, pos, SEEK_SET);
		if (fread(ch, 1, 8, f) != 8) break;
		len = rd32(ch + 4);
		if (!memcmp(ch, "data", 4)) {
			w->dataOff = pos + 8;
			w->dataLen = len;
			if (w->dataOff + w->dataLen > fsize) w->dataLen = fsize - w->dataOff;  /* truncated / bad size field */
			haveData = 1;
		} else if (len <= (1u << 24)) {
			uint8_t *d = (uint8_t *)malloc(len ? len : 1);
			uint32_t got = (uint32_t)fread(d, 1, len, f);
			if (!memcmp(ch, "fmt ", 4) && got >= 16) {
				w->tag = rd16(d); w->channels = rd16(d + 2); w->rate = rd32(d + 4);
				w->blockAlign = rd16(d + 12); w->bits = rd16(d + 14);
				w->fmtLen = got < sizeof(w->fmtRaw) ? got : sizeof(w->fmtRaw);
				memcpy(w->fmtRaw, d, w->fmtLen);
				if (got >= 20 && (w->tag == 0x11 || w->tag == 0x69 || w->tag == 0x02)) w->samplesPerBlock = rd16(d + 18);
				if (w->tag == 0x02 && got >= 22) {
					int i, n = rd16(d + 20);
					if (n > 32) n = 32;
					for (i = 0; i < n && 22u + i * 4 + 4 <= got; i++) {
						w->coef[i][0] = (int16_t)rd16(d + 22 + i * 4);
						w->coef[i][1] = (int16_t)rd16(d + 24 + i * 4);
					}
					w->numCoef = i;
				}
				haveFmt = 1;
			} else if (!memcmp(ch, "fact", 4) && got >= 4) {
				w->hasFact = 1; w->factFrames = rd32(d);
			} else if (!memcmp(ch, "cue ", 4) && ms) {
				parse_cue(ms, d, got);
			} else if (!memcmp(ch, "LIST", 4) && got >= 4 && !memcmp(d, "adtl", 4) && ms) {
				ms->adtlRaw = (uint8_t *)malloc(got); memcpy(ms->adtlRaw, d, got); ms->adtlLen = got;
			} else if (!memcmp(ch, "smpl", 4) && ms) {
				parse_smpl(ms, d, got);
			}
			free(d);
		}
		pos += 8 + (int64_t)len + (len & 1);
	}
	if (ms && ms->adtlRaw && ms->count) parse_adtl(ms, ms->adtlRaw, ms->adtlLen);
	return haveFmt && haveData ? 0 : -1;
}

static const int16_t kMsDefaultCoef[7][2] = { { 256, 0 }, { 512, -256 }, { 0, 0 }, { 192, 64 }, { 240, 0 }, { 460, -208 }, { 392, -232 } };

/* Decodes any supported audio file to the sink. */
static int decode_file(const char *path, Sink *sink, SourceInfo *si, MarkerSet *ms)
{
	FILE *f = xfopen(path, "rb");
	uint8_t head[4096];
	size_t headLen;
	WavIn w;
	int r = -1;
	memset(si, 0, sizeof(*si));
	if (!f) { logf_("  ERROR: cannot open %s", path); return -1; }
	setvbuf(f, NULL, _IOFBF, 1 << 20);
	si->size = file_size(path);
	headLen = fread(head, 1, sizeof(head), f);

	if (parse_riff(f, si->size, &w, ms) == 0) {
		uint16_t tag = w.tag;
		if (tag == 0xFFFE && w.fmtLen >= 26) tag = rd16(w.fmtRaw + 24);   /* SubFormat GUID's first two bytes */
		si->rate = w.rate; si->channels = w.channels; si->bits = w.bits; si->blockAlign = w.blockAlign;
		xfseek(f, w.dataOff, SEEK_SET);

		if ((tag == 1 || tag == 3) && w.blockAlign) {
			/* PCM / float: copy the samples bit-exact with the original fmt chunk */
			OutFormat of;
			uint8_t *b = (uint8_t *)malloc(1 << 20);
			int64_t left = w.dataLen - w.dataLen % w.blockAlign;
			memset(&of, 0, sizeof(of));
			of.tag = w.tag; of.channels = w.channels; of.rate = w.rate; of.bits = w.bits; of.blockAlign = w.blockAlign;
			memcpy(of.raw, w.fmtRaw, w.fmtLen); of.rawLen = w.fmtLen;
			snprintf(si->codec, sizeof(si->codec), "%s %u-bit%s", tag == 1 ? "PCM" : "IEEE float", w.bits, w.tag == 0xFFFE ? " (extensible)" : "");
			while (left > 0 && !sink->failed) {
				size_t want = (size_t)(left < (1 << 20) ? left : (1 << 20));
				size_t got;
				want -= want % w.blockAlign;
				got = fread(b, 1, want, f);
				got -= got % w.blockAlign;
				if (!got) break;
				sink_write(sink, &of, b, got / w.blockAlign);
				left -= got;
			}
			free(b);
			r = sink->failed ? -1 : 0;
		} else if ((tag == 0x11 || tag == 0x69 || tag == 0x02) && w.blockAlign && w.channels >= 1 && w.channels <= 8) {
			/* ADPCM: decode block by block to 16-bit PCM */
			int ms_ = tag == 0x02;
			uint32_t spb = w.samplesPerBlock;
			uint8_t *blk = (uint8_t *)malloc(w.blockAlign);
			int16_t *pcm;
			int64_t left = w.dataLen;
			uint64_t limit = w.hasFact ? w.factFrames : UINT64_MAX, done = 0;
			OutFormat of;
			if (!spb) spb = ms_ ? (uint32_t)((w.blockAlign - 7 * w.channels) * 2 / w.channels + 2)
			                    : (uint32_t)((w.blockAlign - 4 * w.channels) * 2 / w.channels + 1);
			pcm = (int16_t *)malloc(sizeof(int16_t) * (spb + 16) * w.channels);
			memset(&of, 0, sizeof(of));
			of.tag = 1; of.channels = w.channels; of.rate = w.rate; of.bits = 16; of.blockAlign = (uint16_t)(2 * w.channels);
			snprintf(si->codec, sizeof(si->codec), "%s (4-bit, %u samples/block)", ms_ ? "Microsoft ADPCM" : tag == 0x69 ? "Xbox IMA ADPCM" : "IMA ADPCM", spb);
			if (ms_ && !w.numCoef) { memcpy(w.coef, kMsDefaultCoef, sizeof(kMsDefaultCoef)); w.numCoef = 7; }
			while (left > 0 && done < limit && !sink->failed) {
				uint32_t want = left < w.blockAlign ? (uint32_t)left : w.blockAlign;
				uint32_t got = (uint32_t)fread(blk, 1, want, f), frames;
				if (!got) break;
				left -= got;
				frames = ms_ ? msadpcm_block(blk, got, w.channels, spb, (const int16_t (*)[2])w.coef, w.numCoef, pcm)
				             : ima_block(blk, got, w.channels, spb, pcm);
				if (done + frames > limit) frames = (uint32_t)(limit - done);
				sink_write(sink, &of, (const uint8_t *)pcm, frames);
				done += frames;
			}
			if (w.hasFact) snprintf(si->extra, sizeof(si->extra), "fact chunk frame count %u", w.factFrames);
			free(blk); free(pcm);
			r = sink->failed ? -1 : 0;
		} else if (tag == 0x55 || tag == 0x50) {
			r = decode_mpeg(f, w.dataOff, w.dataLen, sink, si);
			snprintf(si->extra, sizeof(si->extra), "MPEG audio inside RIFF/WAVE (format tag 0x%04X)", tag);
		} else {
			logf_("  UNSUPPORTED: WAV format tag 0x%04X in %s", w.tag, path);
		}
	} else if (looks_like_mpeg_audio(head, headLen)) {
		r = decode_mpeg(f, 0, si->size, sink, si);
		if (r != 0 && !sink->opened) logf_("  UNSUPPORTED: not a RIFF/WAVE file and no decodable MPEG audio frames in %s", path);
	} else {
		logf_("  UNSUPPORTED: unrecognised file contents in %s", path);
	}
	fclose(f);
	return r;
}

/* ------------------------------------------------------------------------ */
/* Manifest record                                                           */
/* ------------------------------------------------------------------------ */

static void record(const char *category, const char *title, const char *gameId, const char *srcRel,
                   const char *outRel, const Sink *s, const SourceInfo *si, const MarkerSet *ms,
                   const char *trackRel, const char *extraJson)
{
	double dur = s->fmt.rate ? (double)s->frames / s->fmt.rate : 0;
	char ts[32], outCodec[64];
	int i;
	fmt_time(dur, ts, sizeof(ts));
	snprintf(outCodec, sizeof(outCodec), "%s %u-bit", s->fmt.tag == 3 ? "IEEE float" : "PCM", s->fmt.bits);

	fprintf(g_json, "%s\n  {", g_jsonFirst ? "" : ",");
	g_jsonFirst = 0;
	fprintf(g_json, "\"category\": "); json_str(g_json, category);
	fprintf(g_json, ", \"title\": "); json_str(g_json, title);
	fprintf(g_json, ", \"game_id\": "); json_str(g_json, gameId);
	fprintf(g_json, ",\n   \"source\": {\"file\": "); json_str(g_json, srcRel);
	fprintf(g_json, ", \"bytes\": %lld, \"codec\": ", (long long)si->size); json_str(g_json, si->codec);
	fprintf(g_json, ", \"sample_rate\": %u, \"channels\": %u", si->rate, si->channels);
	if (si->bitrateKbps) fprintf(g_json, ", \"avg_bitrate_kbps\": %u", si->bitrateKbps);
	if (si->hasLame) fprintf(g_json, ", \"encoder_delay_samples\": %u, \"encoder_padding_samples\": %u", si->encDelay, si->encPadding);
	if (si->mismatchedFrames) fprintf(g_json, ", \"skipped_frames_with_different_format\": %llu", (unsigned long long)si->mismatchedFrames);
	if (si->extra[0]) { fprintf(g_json, ", \"note\": "); json_str(g_json, si->extra); }
	fprintf(g_json, "},\n   \"output\": {\"file\": "); json_str(g_json, outRel);
	fprintf(g_json, ", \"codec\": "); json_str(g_json, outCodec);
	fprintf(g_json, ", \"sample_rate\": %u, \"channels\": %u, \"sample_frames\": %llu, \"duration_seconds\": %.6f, \"duration\": \"%s\"}",
		s->fmt.rate, s->fmt.channels, (unsigned long long)s->frames, dur, ts);
	fprintf(g_json, ",\n   \"markers\": [");
	for (i = 0; ms && i < ms->count; i++) {
		char mt[32];
		fmt_time(s->fmt.rate ? (double)ms->m[i].frame / s->fmt.rate : 0, mt, sizeof(mt));
		fprintf(g_json, "%s{\"id\": %u, \"sample_frame\": %u, \"time\": \"%s\", \"label\": ", i ? ", " : "", ms->m[i].id, ms->m[i].frame, mt);
		json_str(g_json, ms->m[i].label);
		if (ms->m[i].note[0]) { fprintf(g_json, ", \"note\": "); json_str(g_json, ms->m[i].note); }
		if (ms->m[i].length) fprintf(g_json, ", \"length_frames\": %u", ms->m[i].length);
		fprintf(g_json, "}");
	}
	fprintf(g_json, "]");
	if (ms && ms->hasSmpl) {
		fprintf(g_json, ", \"smpl_loops\": [");
		for (i = 0; i < (int)ms->smplLoops && i < 8; i++)
			fprintf(g_json, "%s{\"start_frame\": %u, \"end_frame\": %u}", i ? ", " : "", ms->loopStart[i], ms->loopEnd[i]);
		fprintf(g_json, "]");
	}
	if (trackRel) { fprintf(g_json, ", \"track_folder\": "); json_str(g_json, trackRel); }
	if (extraJson) fprintf(g_json, ", %s", extraJson);
	fprintf(g_json, "}");
	fflush(g_json);

	csv_str(g_csv, category); fputc(',', g_csv);
	csv_str(g_csv, title); fputc(',', g_csv);
	csv_str(g_csv, gameId); fputc(',', g_csv);
	csv_str(g_csv, srcRel); fputc(',', g_csv);
	csv_str(g_csv, si->codec); fputc(',', g_csv);
	fprintf(g_csv, "%u,%u,", si->rate, si->channels);
	csv_str(g_csv, outRel); fputc(',', g_csv);
	csv_str(g_csv, outCodec);
	fprintf(g_csv, ",%u,%u,%llu,%.6f,%s,%d\n", s->fmt.rate, s->fmt.channels, (unsigned long long)s->frames, dur, ts, ms ? ms->count : 0);
	fflush(g_csv);

	fprintf(g_txt, "%-14s  %-12s  %s  %-44s  <- %s [%s, %u Hz, %u ch]\n", category, gameId, ts, title, srcRel, si->codec, si->rate, si->channels);
	fflush(g_txt);
}

/* Writes an Audacity label track and a CUE sheet for a stream with markers. */
static void write_marker_files(const char *wavPath, const char *wavName, const MarkerSet *ms, uint32_t rate, uint64_t totalFrames)
{
	char p[PATHMAX];
	FILE *f;
	int i;
	if (!ms || !ms->count || !rate) return;
	snprintf(p, sizeof(p), "%s.labels.txt", wavPath);
	if ((f = xfopen(p, "wb"))) {
		for (i = 0; i < ms->count; i++) {
			double a = (double)ms->m[i].frame / rate;
			double b = ms->m[i].length ? (double)(ms->m[i].frame + ms->m[i].length) / rate
			         : (i + 1 < ms->count ? (double)ms->m[i + 1].frame / rate : (double)totalFrames / rate);
			fprintf(f, "%.6f\t%.6f\t%s\n", a, b, ms->m[i].label[0] ? ms->m[i].label : "marker");
		}
		fclose(f);
	}
	snprintf(p, sizeof(p), "%s.cue", wavPath);
	if ((f = xfopen(p, "wb"))) {
		fprintf(f, "FILE \"%s\" WAVE\n", wavName);
		for (i = 0; i < ms->count && i < 99; i++) {
			/* CUE sheet INDEX uses mm:ss:ff with 75 frames per second */
			uint64_t ff = (uint64_t)ms->m[i].frame * 75 / rate;
			fprintf(f, "  TRACK %02d AUDIO\n    TITLE \"%s\"\n    INDEX 01 %02llu:%02llu:%02llu\n", i + 1,
				ms->m[i].label[0] ? ms->m[i].label : "marker",
				(unsigned long long)(ff / 75 / 60), (unsigned long long)(ff / 75 % 60), (unsigned long long)(ff % 75));
		}
		fclose(f);
	}
}

/* ------------------------------------------------------------------------ */
/* GXT (TEXT\AMERICAN.GXT) lookup for the radio station names               */
/* ------------------------------------------------------------------------ */

typedef struct { uint8_t *data; int64_t len; int64_t keyOff, keyLen, datOff, datLen; } Gxt;

static int gxt_load(const char *gameDir, Gxt *g)
{
	char dir[PATHMAX], path[PATHMAX];
	FILE *f;
	int64_t pos = 0;
	memset(g, 0, sizeof(*g));
	snprintf(dir, sizeof(dir), "%s%cTEXT", gameDir, SEP);
	if (!find_file_ci(dir, "AMERICAN.GXT", path, sizeof(path))) return -1;
	g->len = file_size(path);
	if (g->len <= 0 || g->len > (16 << 20) || !(f = xfopen(path, "rb"))) return -1;
	g->data = (uint8_t *)malloc((size_t)g->len);
	g->len = (int64_t)fread(g->data, 1, (size_t)g->len, f);
	fclose(f);
	while (pos + 8 <= g->len) {
		uint32_t n = rd32(g->data + pos + 4);
		if (pos + 8 + n > g->len) break;
		if (!memcmp(g->data + pos, "TKEY", 4)) { g->keyOff = pos + 8; g->keyLen = n; }
		else if (!memcmp(g->data + pos, "TDAT", 4)) { g->datOff = pos + 8; g->datLen = n; }
		pos += 8 + n;
	}
	return g->keyLen && g->datLen ? 0 : -1;
}

/* Returns the UTF-8 text of a key; non-ASCII characters are UTF-8 encoded as stored. */
static int gxt_get(const Gxt *g, const char *key, char *out, size_t outSize)
{
	int64_t i;
	for (i = 0; i + 12 <= g->keyLen; i += 12) {
		const uint8_t *e = g->data + g->keyOff + i;
		if (strncmp((const char *)e + 4, key, 8) == 0) {
			int64_t o = g->datOff + rd32(e);
			size_t k = 0;
			while (o + 2 <= g->datOff + g->datLen) {
				uint16_t c = rd16(g->data + o);
				o += 2;
				if (!c) break;
				if (c < 0x80) { if (k + 1 < outSize) out[k++] = (char)c; }
				else if (c < 0x800) { if (k + 2 < outSize) { out[k++] = (char)(0xC0 | (c >> 6)); out[k++] = (char)(0x80 | (c & 63)); } }
				else if (k + 3 < outSize) { out[k++] = (char)(0xE0 | (c >> 12)); out[k++] = (char)(0x80 | ((c >> 6) & 63)); out[k++] = (char)(0x80 | (c & 63)); }
			}
			out[k] = 0;
			return k ? 0 : -1;
		}
	}
	return -1;
}

/* ------------------------------------------------------------------------ */
/* Extraction of one streamed file                                           */
/* ------------------------------------------------------------------------ */

static char g_gameDir[PATHMAX], g_outDir[PATHMAX];

static const char *rel(const char *full)
{
	size_t n = strlen(g_gameDir);
	return strncmp(full, g_gameDir, n) == 0 && (full[n] == SEP) ? full + n + 1 : full;
}

static int extract_stream(const char *srcPath, const char *category, const char *subDir, const char *fileStem,
                          const char *title, const char *gameId, int allowSplit, const char *comment)
{
	char dir[PATHMAX], out[PATHMAX], trackDir[PATHMAX], name[PATHMAX], stem[PATHMAX];
	char outRel[PATHMAX], trackRel[PATHMAX];
	Sink sink;
	SourceInfo si;
	MarkerSet ms;
	int r;

	memset(&ms, 0, sizeof(ms));
	snprintf(stem, sizeof(stem), "%s", fileStem);
	sanitize(stem);
	snprintf(dir, sizeof(dir), "%s%c%s", g_outDir, SEP, subDir);
	snprintf(name, sizeof(name), "%s.wav", stem);
	snprintf(out, sizeof(out), "%s%c%s", dir, SEP, name);
	snprintf(trackDir, sizeof(trackDir), "%s%c%s - Tracks", dir, SEP, stem);

	logf_("Extracting %s -> %s", rel(srcPath), rel(out));
	/* markers must be known before the sink is set up, so read the RIFF header first */
	{
		FILE *f = xfopen(srcPath, "rb");
		WavIn w;
		if (f) { parse_riff(f, file_size(srcPath), &w, &ms); fclose(f); }
	}
	sink_init(&sink, out, title, comment, allowSplit ? &ms : NULL, trackDir);
	if (!allowSplit && ms.count) {
		/* still keep the source markers inside the output file */
		if (ms.cueLen) buf_chunk(&sink.extra, "cue ", ms.cueRaw, ms.cueLen);
		if (ms.adtlLen) buf_chunk(&sink.extra, "LIST", ms.adtlRaw, ms.adtlLen);
	}
	/* decode_file re-parses markers; use a scratch set so the sink's sorted set is untouched */
	{
		MarkerSet scratch;
		memset(&scratch, 0, sizeof(scratch));
		r = decode_file(srcPath, &sink, &si, &scratch);
		markers_free(&scratch);
	}
	r = (r == 0 && sink_finish(&sink) == 0) ? 0 : -1;
	if (r == 0) {
		snprintf(outRel, sizeof(outRel), "%s", rel(out));
		snprintf(trackRel, sizeof(trackRel), "%s", rel(trackDir));
		if (ms.count) {
			write_marker_files(out, name, &ms, sink.fmt.rate, sink.frames);
			logf_("  %d cue markers found -> %d track files", ms.count, sink.tracksWritten);
		}
		record(category, title, gameId, rel(srcPath), outRel, &sink, &si, &ms, (allowSplit && ms.count) ? trackRel : NULL, NULL);
		logf_("  OK  %s, %u Hz, %u ch, %llu frames", si.codec, sink.fmt.rate, sink.fmt.channels, (unsigned long long)sink.frames);
		g_countOk++;
	} else {
		logf_("  FAILED %s", rel(srcPath));
		g_countFail++;
	}
	markers_free(&ms);
	return r;
}

/* ------------------------------------------------------------------------ */
/* MPEG program stream (.mpg) audio demux                                    */
/* ------------------------------------------------------------------------ */

typedef struct { FILE *out; char path[PATHMAX]; int64_t bytes; int64_t firstPts; int id; } EsOut;

static int64_t read_pts(const uint8_t *p)
{
	return ((int64_t)((p[0] >> 1) & 7) << 30) | ((int64_t)p[1] << 22) | ((int64_t)(p[2] >> 1) << 15) | ((int64_t)p[3] << 7) | (p[4] >> 1);
}

/* Demuxes all MPEG audio streams (ids 0xC0-0xDF) into elementary stream files. */
static int demux_ps(const char *path, const char *tmpStem, EsOut es[32], int *nes)
{
	FILE *f = xfopen(path, "rb");
	int64_t size = file_size(path), pos = 0;
	uint8_t *buf;
	if (!f) return -1;
	*nes = 0;
	buf = (uint8_t *)malloc(65536 + 16);
	setvbuf(f, NULL, _IOFBF, 1 << 20);
	while (pos + 4 <= size) {
		uint8_t h[4];
		xfseek(f, pos, SEEK_SET);
		if (fread(h, 1, 4, f) != 4) break;
		if (h[0] != 0 || h[1] != 0 || h[2] != 1) { pos++; continue; }   /* resync to a start code */
		if (h[3] == 0xB9) break;                                           /* program end */
		if (h[3] == 0xBA) {                                                /* pack header */
			uint8_t p[10];
			if (fread(p, 1, 10, f) < 1) break;
			if ((p[0] & 0xC0) == 0x40) pos += 14 + (p[9] & 7);           /* MPEG-2 */
			else pos += 12;                                               /* MPEG-1 */
			continue;
		}
		if (h[3] >= 0xBB) {
			uint8_t l[2];
			uint16_t len;
			if (fread(l, 1, 2, f) != 2) break;
			len = (uint16_t)((l[0] << 8) | l[1]);
			if (h[3] >= 0xC0 && h[3] <= 0xDF && len) {
				int i = 0, k;
				int64_t pts = -1;
				size_t got = fread(buf, 1, len, f);
				if (got < len) len = (uint16_t)got;
				if (len >= 3 && (buf[0] & 0xC0) == 0x80) {                /* MPEG-2 PES header */
					if ((buf[1] & 0x80) && len >= 8) pts = read_pts(buf + 3);
					i = 3 + buf[2];
				} else {                                                  /* MPEG-1 PES header */
					while (i < len && buf[i] == 0xFF) i++;
					if (i < len && (buf[i] & 0xC0) == 0x40) i += 2;
					if (i < len && (buf[i] & 0xF0) == 0x20) { if (i + 5 <= len) pts = read_pts(buf + i); i += 5; }
					else if (i < len && (buf[i] & 0xF0) == 0x30) { if (i + 5 <= len) pts = read_pts(buf + i); i += 10; }
					else i += 1;
				}
				for (k = 0; k < *nes && es[k].id != h[3]; k++) {}
				if (k == *nes && *nes < 32) {
					es[k].id = h[3];
					es[k].bytes = 0;
					es[k].firstPts = -1;
					snprintf(es[k].path, sizeof(es[k].path), "%s.stream%02X.partial", tmpStem, h[3]);
					es[k].out = xfopen(es[k].path, "wb");
					(*nes)++;
				}
				if (k < *nes && es[k].out && i < len) {
					if (es[k].firstPts < 0 && pts >= 0) es[k].firstPts = pts;
					fwrite(buf + i, 1, len - i, es[k].out);
					es[k].bytes += len - i;
				}
			}
			pos += 6 + len;
			continue;
		}
		pos++;   /* video slice / sequence start codes inside a PES are skipped by length; stray ones by resync */
	}
	free(buf);
	fclose(f);
	return 0;
}

static void extract_movie(const char *srcPath, const char *movieName)
{
	char stem[PATHMAX], dir[PATHMAX], tmpStem[PATHMAX];
	EsOut es[32];
	int nes = 0, k;
	snprintf(stem, sizeof(stem), "%s", movieName);
	{ char *dot = strrchr(stem, '.'); if (dot) *dot = 0; }
	sanitize(stem);
	snprintf(dir, sizeof(dir), "%s%cIntro Movies", g_outDir, SEP);
	snprintf(tmpStem, sizeof(tmpStem), "%s%c%s", dir, SEP, stem);
	logf_("Demuxing %s", rel(srcPath));
	if (demux_ps(srcPath, tmpStem, es, &nes) != 0 || nes == 0) {
		logf_("  no MPEG audio stream found in %s", rel(srcPath));
		for (k = 0; k < nes; k++) { if (es[k].out) fclose(es[k].out); xremove(es[k].path); }
		return;
	}
	for (k = 0; k < nes; k++) {
		char wavPath[PATHMAX], bitPath[PATHMAX], title[PATHMAX], id[16], suffix[32];
		Sink sink;
		SourceInfo si;
		FILE *f;
		int r = -1;
		if (es[k].out) fclose(es[k].out);
		snprintf(suffix, sizeof(suffix), " (audio stream %02X)", es[k].id);
		snprintf(wavPath, sizeof(wavPath), "%s%s.wav", tmpStem, k ? suffix : "");
		snprintf(title, sizeof(title), "Intro movie audio - %s", stem);
		snprintf(id, sizeof(id), "stream 0x%02X", es[k].id);
		logf_("  audio stream 0x%02X: %lld bytes -> %s", es[k].id, (long long)es[k].bytes, rel(wavPath));
		sink_init(&sink, wavPath, title, "Audio track demuxed from the intro movie", NULL, NULL);
		memset(&si, 0, sizeof(si));
		if ((f = xfopen(es[k].path, "rb"))) {
			r = decode_mpeg(f, 0, es[k].bytes, &sink, &si);
			fclose(f);
		}
		si.size = file_size(srcPath);
		snprintf(si.extra, sizeof(si.extra), "demuxed from MPEG program stream, stream id 0x%02X, %lld bytes of audio bitstream", es[k].id, (long long)es[k].bytes);
		r = (r == 0 && sink_finish(&sink) == 0) ? 0 : -1;
		if (r == 0) {
			const char *ext = si.mpegLayer == 1 ? "mp1" : si.mpegLayer == 2 ? "mp2" : "mp3";
			char ej[PATHMAX * 2 + 300], esc[PATHMAX * 2];
			size_t q = 0, n = 0;
			const char *b;
			snprintf(bitPath, sizeof(bitPath), "%s%s.original.%s", tmpStem, k ? suffix : "", ext);
			if (g_cfg.keepMovieBitstream) xrename(es[k].path, bitPath); else xremove(es[k].path);
			b = rel(bitPath);
			for (; b[q] && n + 2 < sizeof(esc); q++) { if (b[q] == '\\' || b[q] == '"') esc[n++] = '\\'; esc[n++] = b[q]; }
			esc[n] = 0;
			ej[0] = 0;
			if (es[k].firstPts >= 0)
				snprintf(ej, sizeof(ej), "\"first_audio_pts_90khz\": %lld, \"audio_start_in_movie_seconds\": %.6f",
					(long long)es[k].firstPts, es[k].firstPts / 90000.0);
			if (g_cfg.keepMovieBitstream)
				snprintf(ej + strlen(ej), sizeof(ej) - strlen(ej), "%s\"original_bitstream\": \"%s\"", ej[0] ? ", " : "", esc);
			record("Intro Movie", title, id, rel(srcPath), rel(wavPath), &sink, &si, NULL, NULL, ej[0] ? ej : NULL);
			logf_("  OK  %s, %u Hz, %u ch, %llu frames", si.codec, sink.fmt.rate, sink.fmt.channels, (unsigned long long)sink.frames);
			g_countOk++;
		} else {
			xremove(es[k].path);
			logf_("  FAILED audio stream 0x%02X of %s", es[k].id, rel(srcPath));
			g_countFail++;
		}
	}
}

/* ------------------------------------------------------------------------ */
/* SFX.SDT / SFX.RAW                                                         */
/* ------------------------------------------------------------------------ */

static void extract_sfx(const char *audioDir)
{
	char sdtPath[PATHMAX], rawPath[PATHMAX], dir[PATHMAX];
	FILE *fs, *fr;
	int64_t sdtSize, rawSize;
	uint32_t count, i;
	uint8_t *sdt;
	int named;

	if (!find_file_ci(audioDir, "SFX.SDT", sdtPath, sizeof(sdtPath)) || !find_file_ci(audioDir, "SFX.RAW", rawPath, sizeof(rawPath))) {
		logf_("SFX.SDT / SFX.RAW not found, skipping the SFX bank");
		return;
	}
	sdtSize = file_size(sdtPath);
	rawSize = file_size(rawPath);
	count = (uint32_t)(sdtSize / 20);
	named = count == SFX_NAME_COUNT;
	logf_("SFX bank: %u entries in %s (%s)", count, rel(sdtPath), named ? "names from the PC sample list" : "entry count differs from the PC sample list, using numbers only");
	if (!(fs = xfopen(sdtPath, "rb"))) return;
	sdt = (uint8_t *)malloc(count * 20 + 1);
	count = (uint32_t)(fread(sdt, 1, count * 20, fs) / 20);
	fclose(fs);
	if (!(fr = xfopen(rawPath, "rb"))) { free(sdt); return; }
	snprintf(dir, sizeof(dir), "%s%cSFX Bank", g_outDir, SEP);
	xmkdir(dir);

	for (i = 0; i < count; i++) {
		const uint8_t *e = sdt + i * 20;
		uint32_t off = rd32(e), size = rd32(e + 4), rate = rd32(e + 8), loopStart = rd32(e + 12);
		int32_t loopEnd = (int32_t)rd32(e + 16);
		char name[128], path[PATHMAX], gameId[160], json[256];
		OutFormat of;
		Sink sink;
		SourceInfo si;
		uint8_t *data;
		size &= ~1u;
		snprintf(name, sizeof(name), "%s", named ? kSfxNames[i] : "SFX");
		snprintf(path, sizeof(path), "%s%c%04u %s.wav", dir, SEP, i, name);
		snprintf(gameId, sizeof(gameId), "SFX.SDT #%u", i);
		if (!size || (int64_t)off + size > rawSize || !rate) {
			logf_("  SFX #%u (%s): empty or out of range entry, skipped", i, name);
			continue;
		}
		data = (uint8_t *)malloc(size);
		xfseek(fr, off, SEEK_SET);
		if (fread(data, 1, size, fr) != size) { free(data); g_countFail++; continue; }
		memset(&of, 0, sizeof(of));
		of.tag = 1; of.channels = 1; of.rate = rate; of.bits = 16; of.blockAlign = 2;
		sink_init(&sink, path, name, "AUDIO\\SFX.RAW sample (16-bit mono PCM, copied bit-exact)", NULL, NULL);
		sink_write(&sink, &of, data, size / 2);
		free(data);
		memset(&si, 0, sizeof(si));
		snprintf(si.codec, sizeof(si.codec), "PCM 16-bit (SFX.RAW)");
		si.rate = rate; si.channels = 1; si.size = size;
		snprintf(si.extra, sizeof(si.extra), "SFX.RAW byte offset %u, %u bytes", off, size);
		/* SDT loop points are byte offsets into the 16-bit sample; -1 = end of sample */
		if (loopEnd == -1)
			snprintf(json, sizeof(json), "\"loop_start_bytes\": %u, \"loop_end_bytes\": -1, \"loop_start_frame\": %u, \"loop_end_frame\": \"end\"", loopStart, loopStart / 2);
		else
			snprintf(json, sizeof(json), "\"loop_start_bytes\": %u, \"loop_end_bytes\": %d, \"loop_start_frame\": %u, \"loop_end_frame\": %d", loopStart, loopEnd, loopStart / 2, loopEnd / 2);
		if (sink_finish(&sink) == 0) {
			char srcRel[PATHMAX];
			snprintf(srcRel, sizeof(srcRel), "%s", rel(rawPath));
			record("SFX Bank", name, gameId, srcRel, rel(path), &sink, &si, NULL, NULL, json);
			g_countOk++;
		} else g_countFail++;
	}
	fclose(fr);
	free(sdt);
	logf_("SFX bank done");
}

/* ------------------------------------------------------------------------ */
/* Main extraction                                                           */
/* ------------------------------------------------------------------------ */

static const char *kCatDir[] = { "Radio Stations", "Police Radio & Ambience", "Announcements & Intros", "Cutscenes", "Music Cues", "Mission Audio" };
static const char *kCatName[] = { "Radio Station", "Police/Ambience", "Announcement", "Cutscene", "Music Cue", "Mission Audio" };

static const char *strip_prefix(const char *e)
{
	if (!strncmp(e, "STREAMED_SOUND_CUTSCENE_", 24)) return e + 24;
	if (!strncmp(e, "STREAMED_SOUND_", 15)) return e + 15;
	if (!strncmp(e, "SFX_MISSION_", 12)) return e + 12;
	return e;
}

static void pretty(const char *in, char *out, size_t n)
{
	/* "RADIO_POLICE" -> "RADIO POLICE" (names kept exactly as in the game's sound list) */
	size_t k = 0;
	for (; *in && k + 1 < n; in++) out[k++] = *in == '_' ? ' ' : *in;
	out[k] = 0;
}

int ripper_run(const char *gameDir)
{
	char audioDir[PATHMAX], p[PATHMAX], marker[PATHMAX];
	Gxt gxt;
	int haveGxt, i, k;
	DirList dl;
	char *used;
	time_t t0 = time(NULL);

	snprintf(g_gameDir, sizeof(g_gameDir), "%s", gameDir);
	{ size_t n = strlen(g_gameDir); while (n > 1 && (g_gameDir[n - 1] == '\\' || g_gameDir[n - 1] == '/')) g_gameDir[--n] = 0; }
	snprintf(audioDir, sizeof(audioDir), "%s%caudio", g_gameDir, SEP);
	if (!xexists(audioDir)) snprintf(audioDir, sizeof(audioDir), "%s%cAUDIO", g_gameDir, SEP);
	if (!xexists(audioDir)) return -1;

	snprintf(g_outDir, sizeof(g_outDir), "%s%c%s", g_gameDir, SEP, g_cfg.outFolder);
	snprintf(marker, sizeof(marker), "%s%c_extraction_complete.txt", g_outDir, SEP);
	if (!g_cfg.force && xexists(marker)) return 0;   /* already done; delete the marker or set Force=1 to redo */

	if (xmkdir(g_outDir) != 0) return -1;
	for (i = 0; i < 6; i++) { snprintf(p, sizeof(p), "%s%c%s", g_outDir, SEP, kCatDir[i]); xmkdir(p); }
	snprintf(p, sizeof(p), "%s%cOther Audio Files", g_outDir, SEP); if (g_cfg.otherFiles) xmkdir(p);
	snprintf(p, sizeof(p), "%s%cIntro Movies", g_outDir, SEP); if (g_cfg.movies) xmkdir(p);

	snprintf(p, sizeof(p), "%s%cextraction_log.txt", g_outDir, SEP);
	g_log = xfopen(p, "wb");
	snprintf(p, sizeof(p), "%s%cmanifest.json", g_outDir, SEP);
	g_json = xfopen(p, "wb");
	snprintf(p, sizeof(p), "%s%cmanifest.csv", g_outDir, SEP);
	g_csv = xfopen(p, "wb");
	snprintf(p, sizeof(p), "%s%ctracklist.txt", g_outDir, SEP);
	g_txt = xfopen(p, "wb");
	if (!g_json || !g_csv || !g_txt) return -1;
	g_jsonFirst = 1; g_countOk = g_countFail = 0;

	logf_("GTA III Music Ripper %s", RIPPER_VERSION);
	logf_("Game folder: %s", g_gameDir);
	fprintf(g_json, "{\n \"tool\": \"GTA III Music Ripper %s\",\n \"entries\": [", RIPPER_VERSION);
	fprintf(g_csv, "category,title,game_id,source_file,source_codec,source_rate,source_channels,output_file,output_codec,output_rate,output_channels,sample_frames,duration_seconds,duration,cue_markers\n");
	fprintf(g_txt, "GTA III audio extracted by GTA III Music Ripper %s\n", RIPPER_VERSION);
	fprintf(g_txt, "Durations are exact (sample frames / sample rate). Format: category, game id, duration, title, source.\n\n");

	haveGxt = gxt_load(g_gameDir, &gxt) == 0;
	logf_("Station names: %s", haveGxt ? "read from TEXT\\AMERICAN.GXT (keys FEA_FM0-FEA_FM8)" : "AMERICAN.GXT not readable, using file names");

	list_files(audioDir, &dl);
	used = (char *)calloc(dl.count ? dl.count : 1, 1);

	for (i = 0; i < (int)(sizeof(kStreams) / sizeof(kStreams[0])); i++) {
		const StreamDef *s = &kStreams[i];
		char src[PATHMAX], code[64], title[300], stem[400], sub[64];
		if (s->category == CAT_SKIP) continue;
		for (k = 0; k < dl.count; k++)
			if (cmp_ci(dl.names[k], s->file) == 0) { used[k] = 1; break; }
		if (s->category == CAT_MISSION && !g_cfg.missionAudio) continue;
		if (k == dl.count) { logf_("Not present in this copy of the game: AUDIO\\%s (%s)", s->file, s->enumName); continue; }
		snprintf(src, sizeof(src), "%s%c%s", audioDir, SEP, dl.names[k]);
		snprintf(code, sizeof(code), "%s", s->file);
		{ char *dot = strrchr(code, '.'); if (dot) *dot = 0; }
		snprintf(sub, sizeof(sub), "%s", kCatDir[s->category]);

		if (s->category == CAT_RADIO) {
			char key[16], nm[256];
			snprintf(key, sizeof(key), "FEA_FM%d", i);
			if (!(haveGxt && gxt_get(&gxt, key, nm, sizeof(nm)) == 0)) snprintf(nm, sizeof(nm), "%s", code);
			snprintf(title, sizeof(title), "%s", nm);
			snprintf(stem, sizeof(stem), "%02d - %s (%s)", i + 1, nm, code);
			extract_stream(src, kCatName[s->category], sub, stem, title, s->file, 1,
				"Full radio station stream exactly as stored in the game files");
		} else {
			char pr[200];
			pretty(strip_prefix(s->enumName), pr, sizeof(pr));
			snprintf(title, sizeof(title), "%s", pr);
			snprintf(stem, sizeof(stem), "%s - %s", code, pr);
			extract_stream(src, kCatName[s->category], sub, stem, title, s->file, 1, s->enumName);
		}
	}

	/* any other audio in the folder (other releases / extra files) */
	if (g_cfg.otherFiles) {
		for (k = 0; k < dl.count; k++) {
			const char *n = dl.names[k], *dot = strrchr(n, '.');
			char src[PATHMAX], stem[PATHMAX];
			if (used[k] || !dot || (cmp_ci(dot, ".wav") && cmp_ci(dot, ".mp3"))) continue;
			snprintf(src, sizeof(src), "%s%c%s", audioDir, SEP, n);
			snprintf(stem, sizeof(stem), "%.*s", (int)(dot - n), n);
			extract_stream(src, "Other", "Other Audio Files", stem, stem, n, 1, "Audio file in the AUDIO folder that is not in the PC stream table");
		}
	}
	free(used);
	dirlist_free(&dl);
	if (haveGxt) free(gxt.data);

	if (g_cfg.movies) {
		char movDir[PATHMAX];
		snprintf(movDir, sizeof(movDir), "%s%cmovies", g_gameDir, SEP);
		list_files(movDir, &dl);
		for (k = 0; k < dl.count; k++) {
			const char *dot = strrchr(dl.names[k], '.');
			char src[PATHMAX];
			if (!dot || (cmp_ci(dot, ".mpg") && cmp_ci(dot, ".mpeg"))) continue;
			snprintf(src, sizeof(src), "%s%c%s", movDir, SEP, dl.names[k]);
			extract_movie(src, dl.names[k]);
		}
		if (!dl.count) logf_("No movies folder found");
		dirlist_free(&dl);
	}

	if (g_cfg.sfx) extract_sfx(audioDir);

	fprintf(g_json, "\n ],\n \"files_ok\": %d,\n \"files_failed\": %d\n}\n", g_countOk, g_countFail);
	fclose(g_json); fclose(g_csv); fclose(g_txt);
	g_json = g_csv = g_txt = NULL;
	logf_("Finished: %d files written, %d failed, %ld seconds", g_countOk, g_countFail, (long)(time(NULL) - t0));
	{
		/* written even when some files failed, so a damaged file does not trigger a full re-run on every start */
		FILE *m = xfopen(marker, "wb");
		if (m) {
			fprintf(m, "GTA III Music Ripper %s\n%d files written, %d failed (see extraction_log.txt).\n", RIPPER_VERSION, g_countOk, g_countFail);
			fprintf(m, "Delete this file (or set Force=1 in the ini) to extract again.\n");
			fclose(m);
		}
	}
	if (g_log) { fclose(g_log); g_log = NULL; }
	return g_countFail ? 1 : 0;
}

/* ------------------------------------------------------------------------ */
/* Entry points                                                              */
/* ------------------------------------------------------------------------ */

#ifdef _WIN32
static void load_config(const char *iniPath)
{
	wchar_t *w = widen(iniPath);
	wchar_t buf[260];
	g_cfg.enabled = GetPrivateProfileIntW(L"Ripper", L"Enabled", 1, w);
	g_cfg.force = GetPrivateProfileIntW(L"Ripper", L"Force", 0, w);
	g_cfg.mpegFloat = GetPrivateProfileIntW(L"Ripper", L"MpegFloat32", 1, w);
	g_cfg.sfx = GetPrivateProfileIntW(L"Ripper", L"ExtractSfxBank", 1, w);
	g_cfg.missionAudio = GetPrivateProfileIntW(L"Ripper", L"ExtractMissionAudio", 1, w);
	g_cfg.movies = GetPrivateProfileIntW(L"Ripper", L"ExtractMovieAudio", 1, w);
	g_cfg.keepMovieBitstream = GetPrivateProfileIntW(L"Ripper", L"KeepMovieAudioBitstream", 1, w);
	g_cfg.otherFiles = GetPrivateProfileIntW(L"Ripper", L"ExtractOtherAudioFiles", 1, w);
	GetPrivateProfileStringW(L"Ripper", L"OutputFolder", L"Extracted Audio", buf, 260, w);
	narrow(buf, g_cfg.outFolder, sizeof(g_cfg.outFolder));
	sanitize(g_cfg.outFolder);
	if (!g_cfg.outFolder[0]) strcpy(g_cfg.outFolder, "Extracted Audio");
	free(w);
}

static void exe_dir(HMODULE m, char *out, size_t n)
{
	wchar_t w[MAX_PATH * 4];
	wchar_t *s;
	GetModuleFileNameW(m, w, MAX_PATH * 4);
	s = wcsrchr(w, L'\\');
	if (s) *s = 0;
	narrow(w, out, (int)n);
}
#endif

#ifdef RIPPER_ASI
static HMODULE g_self;

static DWORD WINAPI ripper_thread(LPVOID unused)
{
	char gameDir[PATHMAX], ini[PATHMAX], selfPath[PATHMAX];
	wchar_t w[MAX_PATH * 4];
	(void)unused;
	SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
	exe_dir(NULL, gameDir, sizeof(gameDir));
	GetModuleFileNameW(g_self, w, MAX_PATH * 4);
	narrow(w, selfPath, sizeof(selfPath));
	snprintf(ini, sizeof(ini), "%s", selfPath);
	{ char *dot = strrchr(ini, '.'); if (dot) strcpy(dot, ".ini"); }
	load_config(ini);
	if (g_cfg.enabled) ripper_run(gameDir);
	return 0;
}

BOOL WINAPI DllMain(HINSTANCE inst, DWORD reason, LPVOID reserved)
{
	(void)reserved;
	if (reason == DLL_PROCESS_ATTACH) {
		HMODULE pinned;
		g_self = inst;
		DisableThreadLibraryCalls(inst);
		/* Pin the module: some loaders (e.g. Miles' provider scan) unload .asi files that are not
		 * Miles providers, which would otherwise pull the code out from under the worker thread. */
		GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN, (LPCWSTR)(void *)&DllMain, &pinned);
		{
			HANDLE h = CreateThread(NULL, 0, ripper_thread, NULL, 0, NULL);
			if (h) CloseHandle(h);
		}
	}
	return TRUE;
}
#else
int main(int argc, char **argv)
{
	char gameDir[PATHMAX];
	int i, r;
#ifdef _WIN32
	int wargc;
	wchar_t **wargv = CommandLineToArgvW(GetCommandLineW(), &wargc);
	char ini[PATHMAX];
	exe_dir(NULL, gameDir, sizeof(gameDir));
	snprintf(ini, sizeof(ini), "%s\\gta3_music_ripper.ini", gameDir);
	if (xexists(ini)) load_config(ini);
	if (wargv && wargc > 1 && wargv[1][0] != L'-') narrow(wargv[1], gameDir, sizeof(gameDir));
	SetConsoleOutputCP(CP_UTF8);
#else
	snprintf(gameDir, sizeof(gameDir), ".");
	if (argc > 1 && argv[1][0] != '-') snprintf(gameDir, sizeof(gameDir), "%s", argv[1]);
#endif
	for (i = 1; i < argc; i++) {
		if (!strcmp(argv[i], "--force")) g_cfg.force = 1;
		else if (!strcmp(argv[i], "--pcm16")) g_cfg.mpegFloat = 0;
		else if (!strcmp(argv[i], "--no-sfx")) g_cfg.sfx = 0;
	}
	g_cfg.enabled = 1;
	r = ripper_run(gameDir);
	if (r < 0) fprintf(stderr, "No AUDIO folder found in \"%s\". Pass the GTA III folder as the first argument.\n", gameDir);
	return r < 0 ? 2 : r;
}
#endif
