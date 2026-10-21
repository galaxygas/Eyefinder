// Reference tool for the ef-texture tests, built against librw (https://github.com/aap/librw, MIT).
// It is not part of Eyefinder itself; tools/txd-ref/build.sh fetches librw at a pinned commit.
//
//   txdref dump <file.txd>                 one JSON object per texture, as librw reads it
//   txdref ps2gen <out.txd> <rwversion> <seed>
//                                          writes PS2 textures through librw's own PS2 raster code
#include <rw.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cstdint>
#include <string>
#include <vector>

using namespace rw;

// FNV-1a 64: a dependency-free digest for comparing byte buffers with the Rust side.
static uint64_t fnv(const uint8 *p, size_t n){
	uint64_t h = 0xcbf29ce484222325ULL;
	for(size_t i = 0; i < n; i++){ h ^= p[i]; h *= 0x100000001b3ULL; }
	return h;
}

static std::string hex(const std::vector<uint8> &v){
	static const char *d = "0123456789abcdef";
	std::string s;
	for(uint8 b : v){ s += d[b >> 4]; s += d[b & 15]; }
	return s;
}

// PS2 level texels after librw unswizzles them (lock without LOCKNOFETCH). The row pitch of a
// level is its width rounded up to the GS transfer minimum, as in librw's rasterToImage.
// Unlocking always re-swizzles PS2 palettised rasters, so every lock here is paired with exactly
// one unswizzle (a LOCKNOFETCH lock followed by unlock would swizzle the data a second time).
static std::vector<uint8> ps2LevelTexels(Raster *r, int level, int *pw, int *ph){
	ps2::Ps2Raster *natras = GETPS2RASTEREXT(r);
	uint8 *px = r->lock(level, Raster::LOCKREAD);
	int w = r->width, h = r->height;
	*pw = w;
	*ph = h;
	int minw = 1;
	if(r->format & Raster::PAL4) minw = natras->flags & ps2::Ps2Raster::SWIZZLED4 ? 32 : 8;
	else if(r->format & Raster::PAL8) minw = natras->flags & ps2::Ps2Raster::SWIZZLED8 ? 16 : 8;
	int tw = w > minw ? w : minw;
	std::vector<uint8> out;
	for(int y = 0; y < h; y++)
		for(int x = 0; x < w; x++){
			if(r->format & Raster::PAL4){
				uint8 b = px[(y*tw + x)/2];
				out.push_back(x & 1 ? b >> 4 : b & 0xF);
			}else if(r->format & Raster::PAL8)
				out.push_back(px[y*tw + x]);
			else
				for(int k = 0; k < r->depth/8; k++) out.push_back(px[(y*w + x)*(r->depth/8) + k]);
		}
	r->unlock(level);
	return out;
}

static uint32_t rng;
static uint8 rnd(void){ rng = rng*1103515245u + 12345u; return rng >> 16; }

static TexDictionary *readTxd(const char *path){
	StreamFile in;
	if(!in.open(path, "rb")){ fprintf(stderr, "cannot open %s\n", path); return nil; }
	ChunkHeaderInfo h;
	readChunkHeaderInfo(&in, &h);
	if(h.type != ID_TEXDICTIONARY){ fprintf(stderr, "not a TXD\n"); return nil; }
	TexDictionary *txd = TexDictionary::streamRead(&in);
	if(txd == nil) fprintf(stderr, "librw could not read %s\n", path);
	in.close();
	return txd;
}

// librw's PS2 swizzle and unswizzle disagree for levels narrower than the GS transfer minimum, so
// any lock/unlock cycle changes those texels. Each measurement therefore uses its own fresh read:
// the RGBA from toImage first, then the level texels.
static int dump(const char *path){
	TexDictionary *forImage = readTxd(path);
	TexDictionary *txd = readTxd(path);
	if(forImage == nil || txd == nil) return 4;
	std::vector<std::string> images;
	FORLIST(lnk, forImage->textures){
		Image *img = Texture::fromDict(lnk)->raster->toImage();
		if(img == nil){ images.push_back("null"); continue; }
		img->convertTo32();
		std::vector<uint8> rgba;
		for(int y = 0; y < img->height; y++)
			rgba.insert(rgba.end(), img->pixels + y*img->stride, img->pixels + y*img->stride + img->width*4);
		char d[40];
		snprintf(d, sizeof(d), "\"%016llx\"", (unsigned long long)fnv(rgba.data(), rgba.size()));
		images.push_back(std::string(d) + ",\"rgba_hex\":\"" + hex(rgba) + "\"");
		img->destroy();
	}
	size_t ti = 0;
	FORLIST(lnk, txd->textures){
		Texture *t = Texture::fromDict(lnk);
		Raster *r = t->raster;
		printf("{\"name\":\"%s\",\"mask\":\"%s\",\"platform\":%d,\"format\":%d,\"width\":%d,\"height\":%d,\"depth\":%d,\"levels\":[",
		       t->name, t->mask, r->platform, r->format, r->width, r->height, r->depth);
		int n = r->getNumLevels();
		for(int i = 0; i < n; i++){
			if(r->platform == PLATFORM_PS2){
				// texels in raster order (indices for palettised rasters), as librw unswizzles them
				int w, hh;
				std::vector<uint8> texels = ps2LevelTexels(r, i, &w, &hh);
				printf("%s{\"w\":%d,\"h\":%d,\"texels\":\"%016llx\"%s%s%s}", i ? "," : "", w, hh,
				       (unsigned long long)fnv(texels.data(), texels.size()),
				       i == 0 ? ",\"plane\":\"" : "", i == 0 ? hex(texels).c_str() : "", i == 0 ? "\"" : "");
				continue;
			}
			// LOCKNOFETCH: the level bytes exactly as stored in the file
			uint8 *px = r->lock(i, Raster::LOCKREAD|Raster::LOCKNOFETCH);
			int w = r->width, hh = r->height;
			size_t size = r->platform == PLATFORM_XBOX ? xbox::getLevelSize(r, i) : d3d::getLevelSize(r, i);
			printf("%s{\"w\":%d,\"h\":%d,\"size\":%zu,\"fnv\":\"%016llx\"}", i ? "," : "", w, hh, size,
			       (unsigned long long)fnv(px, size));
			r->unlock(i);
		}
		printf("]");
		if(r->platform == PLATFORM_PS2 && r->format & (Raster::PAL4|Raster::PAL8)){
			// palette entries in index order (librw applies the CSM1 swap when locking)
			int entries = r->format & Raster::PAL4 ? 16 : 256;
			int bytes = (r->format & 0xF00) == Raster::C8888 ? 4 : 2;
			uint8 *pal = r->lockPalette(Raster::LOCKREAD);
			std::vector<uint8> v(pal, pal + entries*bytes);
			r->unlockPalette();
			printf(",\"palette\":\"%s\"", hex(v).c_str());
		}
		printf(",\"rgba\":%s", images[ti++].c_str());
		printf("}\n");
	}
	return 0;
}

struct Ps2Case { const char *name; int w, h, depth, format; };

static int ps2gen(const char *out, uint32 version, uint32 seed){
	rng = seed;
	rw::version = version;
	static const Ps2Case cases[] = {
		{ "p8_c8888_64",   64, 64, 8,  Raster::PAL8 | Raster::C8888 },
		{ "p8_c8888_mip",  64, 32, 8,  Raster::PAL8 | Raster::C8888 | Raster::MIPMAP },
		{ "p8_c8888_16x8", 16,  8, 8,  Raster::PAL8 | Raster::C8888 },
		{ "p8_c8888_8x8",   8,  8, 8,  Raster::PAL8 | Raster::C8888 },
		{ "p4_c8888_64",   64, 64, 4,  Raster::PAL4 | Raster::C8888 },
		{ "p4_c8888_mip", 128, 64, 4,  Raster::PAL4 | Raster::C8888 | Raster::MIPMAP },
		{ "p4_c8888_16",   16, 16, 4,  Raster::PAL4 | Raster::C8888 },
		{ "c8888_32",      32, 32, 32, Raster::C8888 },
		{ "c8888_mip",     32, 16, 32, Raster::C8888 | Raster::MIPMAP },
		{ "c1555_32",      32, 32, 16, Raster::C1555 },
		{ "c1555_mip",     64, 64, 16, Raster::C1555 | Raster::MIPMAP },
		{ "p8_big_512",   512, 256, 8, Raster::PAL8 | Raster::C8888 },
		{ "p8_c1555",      32, 32, 8,  Raster::PAL8 | Raster::C1555 },
	};
	TexDictionary *txd = TexDictionary::create();
	for(const Ps2Case &c : cases){
		Raster *r = Raster::create(c.w, c.h, c.depth, c.format | Raster::TEXTURE, PLATFORM_PS2);
		if(r == nil){ fprintf(stderr, "cannot create %s\n", c.name); return 5; }
		int n = r->getNumLevels();
		for(int i = 0; i < n; i++){
			uint8 *px = r->lock(i, Raster::LOCKWRITE|Raster::LOCKNOFETCH);
			int w = r->width, h = r->height;
			size_t bytes = (size_t)w*h*c.depth/8;
			for(size_t k = 0; k < bytes; k++){
				uint8 v = rnd();
				// the GS alpha range is 0..0x80 for 32-bit colours
				if(c.depth == 32 && (c.format & 0xF00) == Raster::C8888 && k % 4 == 3) v %= 0x81;
				px[k] = v;
			}
			r->unlock(i);
		}
		if(c.format & (Raster::PAL4|Raster::PAL8)){
			uint8 *pal = r->lockPalette(Raster::LOCKWRITE|Raster::LOCKNOFETCH);
			int entries = c.format & Raster::PAL4 ? 16 : 256;
			bool c32 = (c.format & 0xF00) == Raster::C8888;
			for(int k = 0; k < entries*(c32 ? 4 : 2); k++){
				uint8 v = rnd();
				if(c32 && k % 4 == 3) v %= 0x81;
				pal[k] = v;
			}
			r->unlockPalette();
		}
		Texture *t = Texture::create(r);
		strncpy(t->name, c.name, 31);
		strncpy(t->mask, "", 31);
		t->setFilter(Texture::LINEAR);
		txd->add(t);
	}
	StreamFile f;
	if(!f.open(out, "wb")) return 6;
	txd->streamWrite(&f);
	f.close();
	return 0;
}

int main(int argc, char **argv){
	Engine::init();
	Engine::open(nil);
	Engine::start();
	if(argc == 3 && strcmp(argv[1], "dump") == 0)
		return dump(argv[2]);
	if(argc == 5 && strcmp(argv[1], "ps2gen") == 0)
		return ps2gen(argv[2], strtoul(argv[3], nil, 0), strtoul(argv[4], nil, 0));
	fprintf(stderr, "usage: txdref dump <txd> | txdref ps2gen <out.txd> <version> <seed>\n");
	return 1;
}
