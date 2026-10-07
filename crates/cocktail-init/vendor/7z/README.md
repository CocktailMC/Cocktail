# Bundled 7-Zip CLI

Cocktail ships a standalone 7-Zip console binary so custom instance packs
(`.7z` / `.zip` / `.tar.gz` / `.xz`) can be extracted without a system install.

| Platform | Binary | Upstream |
|----------|--------|----------|
| Windows x64 | `windows-x64/7za.exe` | [7-Zip Extra 25.01](https://github.com/ip7z/7zip/releases/tag/25.01) `x64/7za.exe` |
| Windows ARM64 | `windows-arm64/7za.exe` | same Extra package `arm64/7za.exe` |
| Linux x64 | `linux-x64/7zzs` | [7-Zip 25.01 linux-x64](https://github.com/ip7z/7zip/releases/tag/25.01) static `7zzs` |

Licensing: 7-Zip Extra is GNU LGPL (see `License.txt`). The CLI is invoked as a
separate process; Cocktail itself remains Apache-2.0.
