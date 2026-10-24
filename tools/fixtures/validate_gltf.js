// Validates a .gltf with the Khronos glTF validator (npm gltf-validator), loading its external files
// from disk: `node tools/fixtures/validate_gltf.js <scene.gltf>` with the package installed (CI map-scene job).
const fs = require('fs');
const path = require('path');
const validator = require('gltf-validator');

const file = process.argv[2];
const dir = path.dirname(file);
validator
  .validateBytes(new Uint8Array(fs.readFileSync(file)), {
    uri: path.basename(file),
    externalResourceFunction: (uri) =>
      new Promise((resolve, reject) => {
        fs.readFile(path.join(dir, decodeURIComponent(uri)), (err, data) => (err ? reject(err.toString()) : resolve(new Uint8Array(data))));
      }),
  })
  .then((r) => {
    const i = r.issues;
    console.log(JSON.stringify({ errors: i.numErrors, warnings: i.numWarnings, infos: i.numInfos, messages: i.messages.slice(0, 15) }, null, 1));
    process.exit(i.numErrors ? 1 : 0);
  })
  .catch((e) => {
    console.error(e);
    process.exit(2);
  });
