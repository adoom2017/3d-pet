# Quaternius Fox asset provenance

- Pack: Ultimate Animated Animal Pack
- Author: Quaternius
- Published: 2021-07
- Official page: <https://quaternius.com/packs/ultimateanimatedanimals.html>
- Official download folder: <https://drive.google.com/drive/folders/1uJ3N5HfB7jKTseJUNQr3N4YaN0UuEtHk>
- Source file: `glTF/Fox.gltf`, Google Drive file ID `1z-CWoUC2vJxrqgGFTYlMaywpE1ooV-bA`
- Retrieved: 2026-08-31
- License: CC0 1.0 Universal; repository copy at `../../LICENSES/Quaternius-Ultimate-Animated-Animals-CC0.txt`

The official `Fox.gltf` is a self-contained JSON glTF with an embedded data-URI buffer.

| Artifact | Size | SHA-256 |
| --- | ---: | --- |
| Official `Fox.gltf` | 3,163,174 bytes | `2f36e3c9c75ecddda85c5f9944e98ee1e88e7c679a546534aff1cea8ecde64c7` |
| Repository `pet.glb` | 1,846,576 bytes | `c2cdd61d1ac40b1aa1a5b621f2ab1a39cf546d23a3a3a30e4cd4001273518870` |
| Official `License.txt` | 364 bytes | `83d8959f9fc56353ed571fbe2dc52e4bcd64508e2399501cd45ac2ce3df0bf8c` |

The repository GLB was produced without geometry or texture optimization:

```bash
npx --yes @gltf-transform/cli@4.4.2 copy Fox.gltf Fox.glb
```

Animation names in the source are:

```text
Attack
Death
Eating
Gallop
Gallop_Jump
Idle
Idle_2
Idle_2_HeadLow
Idle_HitReact1
Idle_HitReact2
Jump_ToIdle
Walk
```
