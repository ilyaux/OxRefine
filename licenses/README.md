# Packaged license and credit texts

`ui/src-tauri/tauri.conf.json` lists the license resources included in desktop bundles. `tools/package-licenses.mjs` copies those same files into the command-line archives. The mapping includes the root MIT license and NOTICE, Apache license/credit texts for the embedded Commons Codec rules and language profiles, the MARC mapping provenance, and the Geist/Geist Mono font licenses from the locked NPM packages.

Additional exact upstream texts retained here:

| File | Source | SHA-256 |
|---|---|---|
| `OpenRefine-3.10.1.txt` | [OpenRefine 3.10.1 BSD license](https://github.com/OpenRefine/OpenRefine/blob/3.10.1/LICENSE.txt), including the bundled GREL descriptions | `24ab0f348a7f5a8f9f5ae3d74083b9330c88477b71682ef7befabfb923c9832b` |
| `RustPython-0.5.0.txt` | [RustPython MIT license](https://github.com/RustPython/RustPython/blob/main/LICENSE); the 0.5.0 crates (rustpython-vm, -stdlib, -pylib) declare MIT and ship no LICENSE file | `0dfeb72ba967794c4ea067b6e67798aefde4fd4cdb10f1882fe1163d2652434d` |
| `python-stdlib-PSF-LICENSE.txt` | `Lib/PSF-LICENSE` from the rustpython-pylib 0.5.0 crate archive: the Python Software Foundation license of the CPython standard library modules frozen into OxRefine | `b9a6d9320b8f2693e8d41e496ce56caadacaddcca9be2a64a61749278f425cf2` |
| `parse_wiki_text-0.1.5.txt` | [parse_wiki_text 0.1.5 LICENSE](https://docs.rs/crate/parse_wiki_text/0.1.5/source/LICENSE) from the crate archive | `3e99b5f4dbc19d8509fa38c5985fc7a6d897963fc664086e4ec56c1c0a52d92a` |

The parse_wiki_text archive SHA-256 is `dd59a245c58efa02bd73c9462fd9d4c28952d650092c6bcba43b60b4707171dd`, also recorded in Cargo.lock. Font files and their license texts are provided by the versions resolved in `ui/package-lock.json` after `npm ci`.

The vendored Metaphone 3 source is attributed in the root NOTICE. Its upstream `metaphone3` 0.2.0 crate declares MIT in Cargo metadata but does not contain a standalone LICENSE file; no replacement copyright statement has been invented here. The inventory above describes the files actually packaged, rather than an exhaustive audit of every transitive Rust or JavaScript dependency.
