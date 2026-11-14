The 70 unmodified normal language profiles in `profiles.zip` come from Optimaize
language-detector 0.6, as distributed with the verified OpenRefine release.
`LICENSE.txt` and `UPSTREAM-README.md` are copied byte-for-byte from that JAR.
The JAR's Maven POM declares Apache License 2.0. The upstream project README's
Legal section also states Apache 2.0 for this work and its predecessor.

Upstream credits Nakatani Shuyo for the original project and language profiles,
Fabian Kessler for the Optimaize rewrite, Francois Roland for the lang-guess fork,
and Robert Theis for additional profiles. The detection implementation also
credits Elmer Garduno. The original NGram source carries the notice:

    Copyright 2011 Nakatani Shuyo

The upstream resource README states that the original training texts for most
profiles are unavailable and appear to have been Wikipedia pages. That statement
is preserved here; no original training texts are included or newly claimed.

`grel/language.rs` adapts the Optimaize detection, n-gram and text-filter algorithms
to Rust. It uses a sparse immutable model and a Java-compatible seeded generator.
The compressed character tables are exhaustive reference API outputs, including
the reference JRE's BMP script classification; the implementation is not bundled.
`provenance.json` records hashes, source version, language order and regeneration
details. Run `python tests/detect_language/profiles.py` to reproduce these assets.

Sources:
- https://github.com/optimaize/language-detector/tree/language-detector-0.6
- https://github.com/optimaize/language-detector/blob/language-detector-0.6/README.md
- https://github.com/optimaize/language-detector/blob/language-detector-0.6/src/main/resources/README.md
