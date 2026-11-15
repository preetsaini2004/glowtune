# Pinned MIT sources

Rust wrapper: https://github.com/colinmarc/signalsmith-stretch-rs/tree/92b8b438c313a727de704c6cf7cabce213e10593

DSP: https://github.com/Signalsmith-Audio/signalsmith-stretch/tree/c5987267e6ad42ff9b620b2802c4600472e85818

FFT: https://github.com/Signalsmith-Audio/linear/tree/157b448e390663e78d845ca9f8dad65140f686ad

Local changes: static-link portable EXPORT macro, checked C ABI declarations replacing build-time bindgen (no libclang required), no Sync marker, release-mode buffer alignment assertions, minimal cc build script. All upstream license notices retained. No Python or runtime installation.

