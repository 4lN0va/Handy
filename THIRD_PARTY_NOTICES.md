# Third-Party Notices

> **Generated file — do not edit by hand.** Regenerate with `node tools/gen_third_party_notices.mjs`.
>
> Source: `src-tauri/Cargo.lock`, audited 2026-10-10T15:47:12.065Z.

This project is a fork of [Handy](https://github.com/cjpais/Handy), which is
licensed under the MIT License, Copyright (c) 2025 CJ Pais. That notice, and
the notices of every dependency below, must be retained in redistributions.

## Scope and limitations

* Covers the **Rust** dependency graph as resolved in `Cargo.lock`.
* **Does not** cover the JavaScript/frontend dependency tree, fonts, or icons.
* **Does not** cover speech **models**, which are downloaded separately and are
  documented in `MODEL_LICENSES.md`.
* Records *declared* licence identifiers. It is not a legal opinion, and it does
  not verify that each obligation has been met.

## Summary

* Registry packages audited: **845**
* Git/path dependencies (not on crates.io): **9**
* Distinct licence expressions: **39**
* Packages with no declared licence: **0**
* Unresolved during audit: **0**

## Git and path dependencies

These are not published on crates.io. They are pinned by commit in
`Cargo.lock`, and **must be vendored before release**, because a deleted or
force-pushed upstream branch would make the project unbuildable.

| Package | Version | Source |
|---|---|---|
| `handy` | 0.9.8 | `(local path)` |
| `handy-voice-api` | 0.1.0 | `(local path)` |
| `hf-hub` | 0.5.0 | `git+https://github.com/cjpais/hf-hub?branch=cancellable-downloads#9918f11ab3473135eb786…` |
| `rdev` | 0.5.0-2 | `git+https://github.com/rustdesk-org/rdev#a90dbe1172f8832f54c97c62e823c5a34af5fdfe` |
| `rodio` | 0.20.1 | `git+https://github.com/cjpais/rodio.git#fed30292db417cb95305c118c0e1d804fb74cbff` |
| `tao` | 0.35.3 | `git+https://github.com/cjpais/tao?rev=c3bee28c1d446d95f08c95c3b6f8d4bde052b876#c3bee28c…` |
| `tao-macros` | 0.1.3 | `git+https://github.com/cjpais/tao?rev=c3bee28c1d446d95f08c95c3b6f8d4bde052b876#c3bee28c…` |
| `tauri-nspanel` | 2.1.0 | `git+https://github.com/ahkohd/tauri-nspanel?branch=v2.1#da9c9a8d4eb7f0524a2508988df1a7d…` |
| `vad-rs` | 0.1.6 | `git+https://github.com/cjpais/vad-rs#2a412ed858695b9251f3f5a1a20d95b59fa7c498` |

## Licence expressions needing attention

These carry obligations beyond plain attribution, or need a human to confirm
compatibility. Where an expression is disjunctive (contains `OR`), selecting a
permissive branch avoids the stricter obligation — that choice must be made
deliberately and recorded.

| Package | Version | License | Permissive option available? |
|---|---|---|---|
| `aho-corasick` | 1.1.4 | `Unlicense OR MIT` | yes |
| `byteorder` | 1.5.0 | `Unlicense OR MIT` | yes |
| `byteorder-lite` | 0.1.0 | `Unlicense OR MIT` | yes |
| `cbindgen` | 0.29.2 | `MPL-2.0` | **no** |
| `clipboard-win` | 5.4.1 | `BSL-1.0` | **no** |
| `cssparser` | 0.29.6 | `MPL-2.0` | **no** |
| `cssparser` | 0.36.0 | `MPL-2.0` | **no** |
| `cssparser-macros` | 0.6.1 | `MPL-2.0` | **no** |
| `dtoa-short` | 0.3.5 | `MPL-2.0` | **no** |
| `dunce` | 1.0.5 | `CC0-1.0 OR MIT-0 OR Apache-2.0` | yes |
| `epoll` | 4.4.0 | `MPL-2.0` | **no** |
| `error-code` | 3.3.2 | `BSL-1.0` | **no** |
| `fst` | 0.4.7 | `Unlicense/MIT` | yes |
| `memchr` | 2.8.0 | `Unlicense OR MIT` | yes |
| `option-ext` | 0.2.0 | `MPL-2.0` | **no** |
| `r-efi` | 5.3.0 | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | yes |
| `ryu` | 1.0.23 | `Apache-2.0 OR BSL-1.0` | yes |
| `same-file` | 1.0.6 | `Unlicense/MIT` | yes |
| `selectors` | 0.24.0 | `MPL-2.0` | **no** |
| `selectors` | 0.36.1 | `MPL-2.0` | **no** |
| `symphonia` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-bundle-flac` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-bundle-mp3` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-codec-aac` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-codec-pcm` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-codec-vorbis` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-core` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-format-isomp4` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-format-ogg` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-format-riff` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-metadata` | 0.5.5 | `MPL-2.0` | **no** |
| `symphonia-utils-xiph` | 0.5.5 | `MPL-2.0` | **no** |
| `termcolor` | 1.4.1 | `Unlicense OR MIT` | yes |
| `walkdir` | 2.5.0 | `Unlicense/MIT` | yes |
| `webpki-root-certs` | 1.0.6 | `CDLA-Permissive-2.0` | **no** |
| `webpki-roots` | 1.0.8 | `CDLA-Permissive-2.0` | **no** |
| `winapi-util` | 0.1.11 | `Unlicense OR MIT` | yes |

### Notes on specific families

* **MPL-2.0** (the `symphonia` audio-decoding family, `cssparser`, `selectors`,
  `cbindgen`, `epoll`, `option-ext`) is *file-level* copyleft. Using these
  unmodified alongside proprietary code is fine; a **modified** MPL file must be
  published under the MPL. Do not patch these dependencies.
* **`r-efi`** offers `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. Select MIT or
  Apache-2.0 so no LGPL obligation applies.
* **`Unlicense OR MIT` / `Unlicense/MIT`** packages: either branch is permissive.
* **BSD-3-Clause / ISC / Zlib / BSL-1.0 / CC0 / CDLA-Permissive-2.0**: permissive;
  retain the notice and licence text.

## Full inventory by licence

### `MIT OR Apache-2.0` (363)

`ahash 0.7.8`, `ahash 0.8.12`, `allocator-api2 0.2.21`, `android_log-sys 0.3.2`, `android_logger 0.15.1`, `anstream 0.6.21`, `anstyle 1.0.14`, `anstyle-parse 0.2.7`, `anstyle-query 1.1.5`, `anstyle-wincon 3.0.11`, `anyhow 1.0.102`, `arbitrary 1.4.2`, `arboard 3.6.1`, `arrayvec 0.7.6`, `async-broadcast 0.7.2`, `async-compression 0.4.43`, `async-recursion 1.1.1`, `async-trait 0.1.89`, `base64 0.21.7`, `base64 0.22.1`, `bitflags 2.11.0`, `block-buffer 0.10.4`, `borsh 1.6.0`, `bumpalo 3.20.2`, `camino 1.2.2`, `cargo-platform 0.1.9`, `cast 0.3.0`, `cc 1.2.56`, `cfg-expr 0.15.8`, `cfg-expr 0.20.6`, `cfg-if 1.0.4`, `chrono 0.4.43`, `clap 4.5.60`, `clap_builder 4.5.60`, `clap_derive 4.5.55`, `clap_lex 1.1.0`, `cmake 0.1.57`, `cocoa-foundation 0.1.2`, `colorchoice 1.0.5`, `compression-codecs 0.4.38`, `compression-core 0.4.32`, `cookie 0.18.1`, `cookie_store 0.22.1`, `core-foundation 0.10.1`, `core-foundation 0.9.4`, `core-foundation-sys 0.8.7`, `core-graphics 0.25.0`, `core-graphics-types 0.1.3`, `core-graphics-types 0.2.0`, `cpufeatures 0.2.17`, `crc32fast 1.5.0`, `crossbeam-channel 0.5.15`, `crossbeam-utils 0.8.21`, `crypto-common 0.1.7`, `dasp_sample 0.11.0`, `deranged 0.5.7`, `derive_arbitrary 1.4.2`, `derive_builder 0.20.2`, `derive_builder_core 0.20.2`, `derive_builder_macro 0.20.2`, `digest 0.10.7`, `dirs 4.0.0`, `dirs 6.0.0`, `dirs-sys 0.3.7`, `dirs-sys 0.5.0`, `displaydoc 0.2.5`, `document-features 0.2.12`, `dtoa 1.0.11`, `dyn-clone 1.0.20`, `earshot 1.2.2`, `embed_plist 1.2.2`, `enum-map 2.7.3`, `enum-map-derive 0.17.0`, `enumflags2 0.7.12`, `enumflags2_derive 0.7.12`, `env_filter 0.1.4`, `env_logger 0.10.2`, `erased-serde 0.4.9`, `errno 0.3.14`, `eyre 0.6.12`, `fdeflate 0.3.7`, `field-offset 0.3.6`, `find-msvc-tools 0.1.9`, `fixedbitset 0.5.7`, `flate2 1.1.9`, `form_urlencoded 1.2.2`, `futures 0.3.32`, `futures-channel 0.3.32`, `futures-core 0.3.32`, `futures-executor 0.3.32`, `futures-io 0.3.32`, `futures-macro 0.3.32`, `futures-sink 0.3.32`, `futures-task 0.3.32`, `futures-util 0.3.32`, `getrandom 0.1.16`, `getrandom 0.2.17`, `getrandom 0.3.4`, `getrandom 0.4.1`, `glob 0.3.3`, `half 2.7.1`, `hashbrown 0.12.3`, `hashbrown 0.14.5`, `hashbrown 0.15.5`, `hashbrown 0.16.1`, `hashlink 0.10.0`, `heck 0.4.1`, `heck 0.5.0`, `hermit-abi 0.5.2`, `hex 0.4.3`, `html5ever 0.29.1`, `html5ever 0.38.0`, `http 1.4.0`, `httparse 1.10.1`, `humantime 2.3.0`, `iana-time-zone 0.1.65`, `iana-time-zone-haiku 0.1.2`, `idna 1.1.0`, `image 0.25.9`, `indenter 0.3.4`, `ipnet 2.11.0`, `iri-string 0.7.10`, `is_terminal_polyfill 1.70.2`, `itoa 1.0.17`, `js-sys 0.3.87`, `jsonptr 0.6.3`, `keyboard-types 0.7.0`, `keyring 3.6.3`, `lazy_static 1.5.0`, `leb128fmt 0.1.0`, `libc 0.2.182`, `litrs 1.0.0`, `lock_api 0.4.14`, `log 0.4.29`, `macos-accessibility-client 0.0.1`, `markup5ever 0.14.1`, `markup5ever 0.38.0`, `match_token 0.1.0`, `memmap2 0.9.10`, `mime 0.3.17`, `native-tls 0.2.18`, `ndarray 0.17.2`, `ndk 0.9.0`, `ndk-context 0.1.1`, `ndk-sys 0.6.0+11769913`, `num-bigint 0.4.6`, `num-complex 0.4.6`, `num-conv 0.2.0`, `num-derive 0.4.2`, `num-integer 0.1.46`, `num-rational 0.4.2`, `num-traits 0.2.19`, `num_cpus 1.17.0`, `num_threads 0.1.7`, `once_cell 1.21.3`, `once_cell_polyfill 1.70.2`, `openssl-probe 0.2.1`, `ordered-stream 0.2.0`, `ort 2.0.0-rc.12`, `ort-sys 2.0.0-rc.12`, `osakit 0.3.1`, `parking_lot 0.12.5`, `parking_lot_core 0.9.12`, `paste 1.0.15`, `pastey 0.1.1`, `percent-encoding 2.3.2`, `petgraph 0.8.3`, `pin-utils 0.1.0`, `piper 0.2.4`, `pkg-config 0.3.32`, `png 0.17.16`, `png 0.18.1`, `powerfmt 0.2.0`, `ppv-lite86 0.2.21`, `prettyplease 0.2.37`, `primal-check 0.3.4`, `proc-macro-crate 1.3.1`, `proc-macro-crate 2.0.2`, `proc-macro-crate 3.4.0`, `proc-macro-error 1.0.4`, `proc-macro-error-attr 1.0.4`, `proc-macro-hack 0.5.20+deprecated`, `proc-macro2 1.0.106`, `quote 1.0.44`, `rand 0.7.3`, `rand 0.8.5`, `rand 0.9.4`, `rand_chacha 0.2.2`, `rand_chacha 0.3.1`, `rand_chacha 0.9.0`, `rand_core 0.5.1`, `rand_core 0.6.4`, `rand_core 0.9.5`, `rand_pcg 0.2.1`, `ref-cast 1.0.25`, `ref-cast-impl 1.0.25`, `regex 1.12.3`, `regex-automata 0.4.14`, `regex-syntax 0.8.9`, `reqwest 0.12.28`, `reqwest 0.13.2`, `rtrb 0.4.0`, `rustc_version 0.4.1`, `rustfft 6.4.1`, `rustls-pki-types 1.14.0`, `rustls-platform-verifier 0.6.2`, `rustls-platform-verifier-android 0.1.1`, `rustversion 1.0.22`, `scopeguard 1.2.0`, `security-framework 3.7.0`, `security-framework-sys 2.17.0`, `semver 1.0.27`, `serde 1.0.228`, `serde-untagged 0.1.9`, `serde_core 1.0.228`, `serde_derive 1.0.228`, `serde_derive_internals 0.29.1`, `serde_json 1.0.149`, `serde_repr 0.1.20`, `serde_spanned 0.6.9`, `serde_spanned 1.0.4`, `serde_with 3.16.1`, `serde_with_macros 3.16.1`, `serialize-to-javascript 0.1.2`, `serialize-to-javascript-impl 0.1.2`, `servo_arc 0.2.0`, `servo_arc 0.4.3`, `sha2 0.10.9`, `shlex 1.3.0`, `signal-hook-registry 1.4.8`, `simdutf8 0.1.5`, `smallvec 1.15.1`, `socket2 0.6.2`, `softbuffer 0.4.8`, `stable_deref_trait 1.2.1`, `strength_reduce 0.2.4`, `string_cache 0.8.9`, `string_cache 0.9.0`, `string_cache_codegen 0.5.4`, `string_cache_codegen 0.6.1`, `swift-rs 1.0.7`, `syn 1.0.109`, `syn 2.0.117`, `sys-locale 0.3.2`, `system-configuration 0.7.0`, `system-configuration-sys 0.6.0`, `system-deps 6.2.2`, `system-deps 7.0.7`, `tar 0.4.44`, `tempfile 3.25.0`, `tendril 0.5.1`, `thiserror 1.0.69`, `thiserror 2.0.18`, `thiserror-impl 1.0.69`, `thiserror-impl 2.0.18`, `time 0.3.47`, `time-core 0.1.8`, `time-macros 0.2.27`, `tokio-rustls 0.26.4`, `toml 0.8.2`, `toml 0.9.12+spec-1.1.0`, `toml_datetime 0.6.3`, `toml_datetime 0.7.5+spec-1.1.0`, `toml_edit 0.19.15`, `toml_edit 0.20.2`, `toml_edit 0.23.10+spec-1.0.0`, `toml_parser 1.0.9+spec-1.1.0`, `toml_writer 1.0.6+spec-1.1.0`, `transpose 0.2.3`, `tray-icon 0.24.1`, `typeid 1.0.3`, `typenum 1.19.0`, `unicode-segmentation 1.12.0`, `unicode-width 0.2.2`, `unicode-xid 0.2.6`, `unty 0.0.4`, `ureq 3.2.0`, `ureq-proto 0.5.3`, `url 2.5.8`, `utf-8 0.7.6`, `wasm-bindgen 0.2.110`, `wasm-bindgen-futures 0.4.60`, `wasm-bindgen-macro 0.2.110`, `wasm-bindgen-macro-support 0.2.110`, `wasm-bindgen-shared 0.2.110`, `wasm-bindgen-test 0.3.60`, `wasm-bindgen-test-macro 0.3.60`, `wasm-bindgen-test-shared 0.2.110`, `wasm-streams 0.4.2`, `wasm-streams 0.5.0`, `web-sys 0.3.87`, `web-time 1.1.0`, `web_atoms 0.2.5`, `weezl 0.1.12`, `widestring 1.2.1`, `windows 0.54.0`, `windows 0.58.0`, `windows 0.61.3`, `windows-collections 0.2.0`, `windows-core 0.54.0`, `windows-core 0.58.0`, `windows-core 0.61.2`, `windows-future 0.2.1`, `windows-implement 0.58.0`, `windows-implement 0.60.2`, `windows-interface 0.58.0`, `windows-interface 0.59.3`, `windows-link 0.1.3`, `windows-link 0.2.1`, `windows-numerics 0.2.0`, `windows-registry 0.6.1`, `windows-result 0.1.2`, `windows-result 0.2.0`, `windows-result 0.3.4`, `windows-result 0.4.1`, `windows-strings 0.1.0`, `windows-strings 0.4.2`, `windows-strings 0.5.1`, `windows-sys 0.45.0`, `windows-sys 0.48.0`, `windows-sys 0.52.0`, `windows-sys 0.59.0`, `windows-sys 0.60.2`, `windows-sys 0.61.2`, `windows-targets 0.42.2`, `windows-targets 0.48.5`, `windows-targets 0.52.6`, `windows-targets 0.53.5`, `windows-threading 0.1.0`, `windows-version 0.1.7`, `windows_aarch64_gnullvm 0.42.2`, `windows_aarch64_gnullvm 0.48.5`, `windows_aarch64_gnullvm 0.52.6`, `windows_aarch64_gnullvm 0.53.1`, `windows_aarch64_msvc 0.42.2`, `windows_aarch64_msvc 0.48.5`, `windows_aarch64_msvc 0.52.6`, `windows_aarch64_msvc 0.53.1`, `windows_i686_gnu 0.42.2`, `windows_i686_gnu 0.48.5`, `windows_i686_gnu 0.52.6`, `windows_i686_gnu 0.53.1`, `windows_i686_gnullvm 0.52.6`, `windows_i686_gnullvm 0.53.1`, `windows_i686_msvc 0.42.2`, `windows_i686_msvc 0.48.5`, `windows_i686_msvc 0.52.6`, `windows_i686_msvc 0.53.1`, `windows_x86_64_gnu 0.42.2`, `windows_x86_64_gnu 0.48.5`, `windows_x86_64_gnu 0.52.6`, `windows_x86_64_gnu 0.53.1`, `windows_x86_64_gnullvm 0.42.2`, `windows_x86_64_gnullvm 0.48.5`, `windows_x86_64_gnullvm 0.52.6`, `windows_x86_64_gnullvm 0.53.1`, `windows_x86_64_msvc 0.42.2`, `windows_x86_64_msvc 0.48.5`, `windows_x86_64_msvc 0.52.6`, `windows_x86_64_msvc 0.53.1`, `x11rb 0.13.2`, `x11rb-protocol 0.13.2`, `xattr 1.6.1`

### `MIT` (217)

`alsa-sys 0.3.1`, `atk 0.18.2`, `atk-sys 0.18.2`, `auto-launch 0.5.0`, `bincode 2.0.1`, `bincode_derive 2.0.1`, `bitvec 1.0.1`, `block 0.1.6`, `block2 0.6.2`, `byte-unit 5.2.0`, `bytecheck 0.6.12`, `bytecheck_derive 0.6.12`, `bytes 1.11.1`, `cairo-rs 0.18.5`, `cairo-sys-rs 0.18.2`, `cargo_metadata 0.19.2`, `cfb 0.7.3`, `cfg_aliases 0.2.1`, `combine 4.6.7`, `console 0.16.3`, `convert_case 0.4.0`, `crunchy 0.2.4`, `darling 0.20.11`, `darling 0.21.3`, `darling_core 0.20.11`, `darling_core 0.21.3`, `darling_macro 0.20.11`, `darling_macro 0.21.3`, `derive_more 0.99.20`, `derive_more 2.1.1`, `derive_more-impl 2.1.1`, `dispatch 0.2.0`, `dlopen2 0.8.2`, `dlopen2_derive 0.4.3`, `dom_query 0.27.0`, `embed-resource 3.0.6`, `endi 1.1.1`, `enigo 0.6.1`, `extended 0.1.0`, `fax 0.2.6`, `fax_derive 0.2.0`, `fern 0.7.1`, `funty 2.0.0`, `gdk 0.18.2`, `gdk-pixbuf 0.18.5`, `gdk-pixbuf-sys 0.18.0`, `gdk-sys 0.18.2`, `gdkwayland-sys 0.18.2`, `gdkx11 0.18.2`, `gdkx11-sys 0.18.2`, `generic-array 0.14.7`, `gio 0.18.4`, `gio-sys 0.18.1`, `glib 0.18.5`, `glib-macros 0.18.5`, `glib-sys 0.18.1`, `gobject-sys 0.18.0`, `gtk 0.18.2`, `gtk-layer-shell 0.8.2`, `gtk-layer-shell-sys 0.7.2`, `gtk-sys 0.18.2`, `gtk3-macros 0.18.2`, `h2 0.4.13`, `handy-keys 0.3.4`, `http-body 1.0.1`, `http-body-util 0.1.3`, `http-range 0.1.5`, `hyper 1.8.1`, `hyper-util 0.1.20`, `ico 0.5.0`, `indicatif 0.18.4`, `infer 0.19.0`, `is-docker 0.2.0`, `is-terminal 0.4.17`, `is-wsl 0.4.0`, `javascriptcore-rs 1.1.2`, `javascriptcore-rs-sys 1.1.1`, `kuchikiki 0.8.8-speedreader`, `libm 0.2.16`, `libredox 0.1.12`, `libsqlite3-sys 0.35.0`, `malloc_buf 0.0.6`, `matches 0.1.10`, `memoffset 0.9.1`, `minisign-verify 0.2.4`, `mio 0.8.11`, `mio 1.1.1`, `natural 0.5.0`, `new_debug_unreachable 1.0.6`, `nix 0.29.0`, `nix 0.30.1`, `nom 8.0.0`, `nu-ansi-term 0.50.3`, `objc 0.2.7`, `objc2 0.6.4`, `objc2-encode 4.1.0`, `objc2-foundation 0.3.2`, `oorandom 11.1.5`, `open 5.3.3`, `openssl-sys 0.9.111`, `os_info 3.14.0`, `os_pipe 1.2.3`, `pango 0.18.3`, `pango-sys 0.18.0`, `phf 0.10.1`, `phf 0.11.3`, `phf 0.13.1`, `phf 0.8.0`, `phf_codegen 0.11.3`, `phf_codegen 0.13.1`, `phf_codegen 0.8.0`, `phf_generator 0.10.0`, `phf_generator 0.11.3`, `phf_generator 0.13.1`, `phf_generator 0.8.0`, `phf_macros 0.10.0`, `phf_macros 0.13.1`, `phf_shared 0.10.0`, `phf_shared 0.11.3`, `phf_shared 0.13.1`, `phf_shared 0.8.0`, `plist 1.8.0`, `precomputed-hash 0.1.1`, `ptr_meta 0.1.4`, `ptr_meta_derive 0.1.4`, `quick-xml 0.38.4`, `radium 0.7.0`, `realfft 3.5.0`, `redox_syscall 0.5.18`, `redox_syscall 0.7.1`, `redox_users 0.4.6`, `redox_users 0.5.2`, `rend 0.4.2`, `rfd 0.16.0`, `ringbuffer 0.15.0`, `rkyv 0.7.46`, `rkyv_derive 0.7.46`, `rubato 0.16.2`, `rusqlite 0.37.0`, `rust_decimal 1.40.0`, `schannel 0.1.28`, `schemars 0.8.22`, `schemars 0.9.0`, `schemars 1.2.1`, `schemars_derive 0.8.22`, `seahash 4.1.0`, `simd-adler32 0.3.8`, `slab 0.4.12`, `soup3 0.5.0`, `soup3-sys 0.5.0`, `specta 2.0.0-rc.22`, `specta-macros 2.0.0-rc.18`, `specta-serde 0.0.9`, `specta-typescript 0.0.9`, `strsim 0.11.1`, `strum 0.24.1`, `strum_macros 0.24.3`, `synstructure 0.13.2`, `tap 1.0.1`, `tauri-plugin-macos-permissions 2.3.0`, `tauri-specta 2.0.0-rc.21`, `tauri-specta-macros 2.0.0-rc.16`, `tauri-winres 0.3.5`, `tiff 0.10.3`, `tokio 1.49.0`, `tokio-macros 2.6.0`, `tokio-native-tls 0.3.1`, `tokio-util 0.7.18`, `tower 0.5.3`, `tower-http 0.6.8`, `tower-layer 0.3.3`, `tower-service 0.3.3`, `tracing 0.1.44`, `tracing-attributes 0.1.31`, `tracing-core 0.1.36`, `transcribe-cpp 0.3.1`, `transcribe-cpp-sys 0.3.1`, `transcribe-rs 0.3.8`, `tree_magic_mini 3.2.2`, `try-lock 0.2.5`, `uds_windows 1.1.0`, `unit-prefix 0.5.2`, `urlpattern 0.3.0`, `utf8-width 0.1.8`, `version-compare 0.2.1`, `virtue 0.0.18`, `vswhom 0.1.0`, `vswhom-sys 0.1.3`, `want 0.3.1`, `wayland-backend 0.3.12`, `wayland-client 0.31.12`, `wayland-protocols 0.32.10`, `wayland-protocols-wlr 0.3.10`, `wayland-scanner 0.31.8`, `wayland-sys 0.31.8`, `webkit2gtk 2.0.2`, `webkit2gtk-sys 2.0.2`, `webview2-com 0.38.2`, `webview2-com-macros 0.8.1`, `webview2-com-sys 0.38.2`, `whatlang 0.16.4`, `winnow 0.5.40`, `winnow 0.7.14`, `winreg 0.10.1`, `winreg 0.55.0`, `wyz 0.5.1`, `x11 2.21.0`, `x11-dl 2.21.0`, `xkbcommon 0.9.0`, `zbus 5.13.2`, `zbus_macros 5.13.2`, `zbus_names 4.3.1`, `zip 4.6.1`, `zmij 1.0.21`, `zvariant 5.9.2`, `zvariant_derive 5.9.2`, `zvariant_utils 3.3.0`

### `Apache-2.0 OR MIT` (68)

`async-channel 2.5.0`, `async-executor 1.14.0`, `async-io 2.6.0`, `async-lock 3.4.2`, `async-process 2.5.0`, `async-signal 0.2.13`, `async-task 4.7.1`, `atomic-waker 1.1.2`, `autocfg 1.5.0`, `base64ct 1.8.3`, `bit-set 0.8.0`, `bit-vec 0.8.0`, `blocking 1.6.2`, `cargo_toml 0.22.3`, `concurrent-queue 2.5.0`, `ctor 0.8.0`, `ctor-proc-macro 0.0.7`, `der 0.7.10`, `dtor 0.3.0`, `dtor-proc-macro 0.0.6`, `encode_unicode 1.0.0`, `equivalent 1.0.2`, `evdev 0.13.2`, `event-listener 5.4.1`, `event-listener-strategy 0.5.4`, `fastrand 2.3.0`, `futures-lite 2.6.1`, `global-hotkey 0.8.0`, `idna_adapter 1.2.1`, `indexmap 1.9.3`, `indexmap 2.13.0`, `libappindicator 0.9.0`, `libappindicator-sys 0.9.0`, `muda 0.19.3`, `parking 2.2.1`, `pem-rfc7468 0.7.0`, `pin-project-lite 0.2.16`, `polling 3.11.0`, `portable-atomic 1.13.1`, `portable-atomic-util 0.2.5`, `rustc-hash 2.1.3`, `tauri 2.11.5`, `tauri-build 2.6.3`, `tauri-codegen 2.6.3`, `tauri-macros 2.6.3`, `tauri-plugin 2.5.3`, `tauri-plugin-autostart 2.5.1`, `tauri-plugin-clipboard-manager 2.3.2`, `tauri-plugin-dialog 2.6.0`, `tauri-plugin-fs 2.4.5`, `tauri-plugin-global-shortcut 2.3.2`, `tauri-plugin-log 2.8.0`, `tauri-plugin-opener 2.5.3`, `tauri-plugin-os 2.3.2`, `tauri-plugin-process 2.3.1`, `tauri-plugin-single-instance 2.4.0`, `tauri-plugin-store 2.4.2`, `tauri-plugin-updater 2.10.1`, `tauri-runtime 2.11.3`, `tauri-runtime-wry 2.11.4`, `tauri-utils 2.9.3`, `utf8_iter 1.0.4`, `utf8parse 0.2.2`, `uuid 1.21.0`, `value-bag 1.12.0`, `window-vibrancy 0.6.0`, `wry 0.55.1`, `zeroize 1.8.2`

### `MIT/Apache-2.0` (42)

`android_system_properties 0.1.5`, `bitflags 1.3.2`, `coreaudio-rs 0.13.0`, `downcast-rs 1.2.1`, `fallible-iterator 0.3.0`, `fallible-streaming-iterator 0.1.9`, `filetime 0.2.27`, `foreign-types 0.3.2`, `foreign-types 0.5.0`, `foreign-types-macros 0.2.3`, `foreign-types-shared 0.1.1`, `foreign-types-shared 0.3.1`, `hyper-tls 0.6.0`, `id-arena 2.3.0`, `ident_case 1.0.1`, `jni 0.21.1`, `jni-sys 0.3.0`, `json-patch 3.0.1`, `mac 0.1.1`, `matrixmultiply 0.3.10`, `nodrop 0.1.14`, `openssl-macros 0.1.1`, `pathdiff 0.2.3`, `quick-error 2.0.1`, `rand_hc 0.2.0`, `rawpointer 0.2.1`, `serde_urlencoded 0.7.1`, `siphasher 0.3.11`, `siphasher 1.0.2`, `socks 0.3.4`, `tendril 0.4.3`, `unic-char-property 0.9.0`, `unic-char-range 0.9.0`, `unic-common 0.9.0`, `unic-ucd-ident 0.9.0`, `unic-ucd-version 0.9.0`, `vcpkg 0.2.15`, `version_check 0.9.5`, `winapi 0.3.9`, `winapi-i686-pc-windows-gnu 0.4.0`, `winapi-x86_64-pc-windows-gnu 0.4.0`, `wl-clipboard-rs 0.9.3`

### `Zlib OR Apache-2.0 OR MIT` (25)

`bytemuck 1.25.0`, `dispatch2 0.3.0`, `objc2-app-kit 0.3.2`, `objc2-audio-toolbox 0.3.2`, `objc2-cloud-kit 0.3.2`, `objc2-core-audio 0.3.2`, `objc2-core-audio-types 0.3.2`, `objc2-core-data 0.3.2`, `objc2-core-foundation 0.3.2`, `objc2-core-graphics 0.3.2`, `objc2-core-image 0.3.2`, `objc2-core-location 0.3.2`, `objc2-core-text 0.3.2`, `objc2-core-video 0.3.2`, `objc2-exception-helper 0.1.1`, `objc2-io-surface 0.3.2`, `objc2-metal 0.3.2`, `objc2-osa-kit 0.3.2`, `objc2-quartz-core 0.3.2`, `objc2-security 0.3.2`, `objc2-service-management 0.3.2`, `objc2-ui-kit 0.3.2`, `objc2-user-notifications 0.3.2`, `objc2-web-kit 0.3.2`, `tinyvec 1.10.0`

### `MPL-2.0` (21)

`cbindgen 0.29.2`, `cssparser 0.29.6`, `cssparser 0.36.0`, `cssparser-macros 0.6.1`, `dtoa-short 0.3.5`, `epoll 4.4.0`, `option-ext 0.2.0`, `selectors 0.24.0`, `selectors 0.36.1`, `symphonia 0.5.5`, `symphonia-bundle-flac 0.5.5`, `symphonia-bundle-mp3 0.5.5`, `symphonia-codec-aac 0.5.5`, `symphonia-codec-pcm 0.5.5`, `symphonia-codec-vorbis 0.5.5`, `symphonia-core 0.5.5`, `symphonia-format-isomp4 0.5.5`, `symphonia-format-ogg 0.5.5`, `symphonia-format-riff 0.5.5`, `symphonia-metadata 0.5.5`, `symphonia-utils-xiph 0.5.5`

### `Unicode-3.0` (18)

`icu_collections 2.1.1`, `icu_locale_core 2.1.1`, `icu_normalizer 2.1.1`, `icu_normalizer_data 2.1.1`, `icu_properties 2.1.2`, `icu_properties_data 2.1.2`, `icu_provider 2.1.1`, `litemap 0.8.1`, `potential_utf 0.1.4`, `tinystr 0.8.2`, `writeable 0.6.2`, `yoke 0.8.1`, `yoke-derive 0.8.1`, `zerofrom 0.1.6`, `zerofrom-derive 0.1.6`, `zerotrie 0.2.3`, `zerovec 0.11.5`, `zerovec-derive 0.11.2`

### `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` (15)

`linux-raw-sys 0.11.0`, `rustix 1.1.3`, `wasi 0.11.1+wasi-snapshot-preview1`, `wasi 0.9.0+wasi-snapshot-preview1`, `wasip2 1.0.2+wasi-0.2.9`, `wasip3 0.4.0+wasi-0.3.0-rc-2026-01-06`, `wasm-encoder 0.244.0`, `wasm-metadata 0.244.0`, `wasmparser 0.244.0`, `wit-bindgen 0.51.0`, `wit-bindgen-core 0.51.0`, `wit-bindgen-rust 0.51.0`, `wit-bindgen-rust-macro 0.51.0`, `wit-component 0.244.0`, `wit-parser 0.244.0`

### `Apache-2.0` (11)

`borsh-derive 1.6.0`, `cpal 0.16.0`, `ferrous-opencc 0.2.3`, `ferrous-opencc-compiler 0.1.0`, `gethostname 1.1.0`, `hound 3.5.1`, `isolang 2.4.0`, `lzma-rust2 0.15.7`, `openssl 0.10.75`, `rusqlite_migration 2.3.0`, `sync_wrapper 1.0.2`

### `Apache-2.0/MIT` (7)

`alsa 0.9.1`, `cesu8 1.1.0`, `dbus 0.9.12`, `fxhash 0.2.1`, `libdbus-sys 0.2.7`, `minicov 0.3.8`, `signal-hook 0.3.18`

### `ISC` (6)

`hmac-sha256 1.1.14`, `inotify 0.10.2`, `inotify-sys 0.1.5`, `libloading 0.7.4`, `rustls-webpki 0.103.9`, `untrusted 0.9.0`

### `Unlicense OR MIT` (6)

`aho-corasick 1.1.4`, `byteorder 1.5.0`, `byteorder-lite 0.1.0`, `memchr 2.8.0`, `termcolor 1.4.1`, `winapi-util 0.1.11`

### `MIT OR Apache-2.0 OR Zlib` (5)

`raw-window-handle 0.6.2`, `tinyvec_macros 0.1.1`, `xkeysym 0.2.1`, `zune-core 0.4.12`, `zune-jpeg 0.4.21`

### `Apache-2.0 OR ISC OR MIT` (3)

`hyper-rustls 0.27.7`, `rustls 0.23.36`, `rustls-native-certs 0.8.3`

### `BSD-3-Clause` (3)

`alloc-no-stdlib 2.0.4`, `alloc-stdlib 0.2.2`, `subtle 2.6.1`

### `MIT / Apache-2.0` (3)

`cocoa 0.24.1`, `core-graphics 0.22.3`, `futf 0.1.5`

### `Unlicense/MIT` (3)

`fst 0.4.7`, `same-file 1.0.6`, `walkdir 2.5.0`

### `Apache-2.0 WITH LLVM-exception` (2)

`target-lexicon 0.12.16`, `target-lexicon 0.13.3`

### `BSD-2-Clause OR Apache-2.0 OR MIT` (2)

`zerocopy 0.8.39`, `zerocopy-derive 0.8.39`

### `BSD-3-Clause OR Apache-2.0` (2)

`moxcms 0.7.11`, `pxfm 0.1.27`

### `BSD-3-Clause OR MIT OR Apache-2.0` (2)

`num_enum 0.7.5`, `num_enum_derive 0.7.5`

### `BSL-1.0` (2)

`clipboard-win 5.4.1`, `error-code 3.3.2`

### `CDLA-Permissive-2.0` (2)

`webpki-root-certs 1.0.6`, `webpki-roots 1.0.8`

### `Zlib` (2)

`foldhash 0.1.5`, `foldhash 0.2.0`

### `(Apache-2.0 OR MIT) AND BSD-3-Clause` (1)

`encoding_rs 0.8.35`

### `(MIT OR Apache-2.0) AND Unicode-3.0` (1)

`unicode-ident 1.0.24`

### `0BSD OR MIT OR Apache-2.0` (1)

`adler2 2.0.1`

### `Apache-2.0 / MIT` (1)

`fnv 1.0.7`

### `Apache-2.0 AND ISC` (1)

`ring 0.17.14`

### `Apache-2.0 AND MIT` (1)

`dpi 0.1.2`

### `Apache-2.0 OR BSL-1.0` (1)

`ryu 1.0.23`

### `BSD-2-Clause` (1)

`Inflector 0.11.4`

### `BSD-2-Clause OR MIT OR Apache-2.0` (1)

`mach2 0.4.3`

### `BSD-3-Clause AND MIT` (1)

`brotli 8.0.2`

### `BSD-3-Clause/MIT` (1)

`brotli-decompressor 5.0.0`

### `CC0-1.0 OR MIT-0 OR Apache-2.0` (1)

`dunce 1.0.5`

### `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (1)

`r-efi 5.3.0`

### `MIT OR Zlib OR Apache-2.0` (1)

`miniz_oxide 0.8.9`

### `MIT/BSD-3-Clause` (1)

`rust-stemmers 1.2.0`

## Non-Rust components

These are linked or invoked but do not appear as crates. Each needs its own
notice, and the list is maintained by hand because it cannot be derived from
`Cargo.lock` (a `-sys` binding crate does not carry the C library's licence).

| Component | License | Note |
|---|---|---|
| whisper.cpp / ggml | MIT | via `transcribe-cpp` |
| ONNX Runtime | MIT | via `ort` / `ort-sys` |
| Silero VAD (model + runtime) | MIT | via `vad-rs`; **the `vad-rs` repository ships no LICENSE file** — see LICENSE_AUDIT.md |
| SQLite | Public domain | bundled via `libsqlite3-sys` |
| OpenBLAS | BSD-3-Clause | Linux CPU backend |
| GTK 3 (C library) | LGPL-2.1-or-later | Linux; must remain dynamically linked and replaceable |
| WebKitGTK (C library) | LGPL-2.1-or-later + BSD | Linux; same requirement |
| gtk-layer-shell | MIT | Linux overlay |
| libopus | BSD-3-Clause | only if the `opus` crate is enabled |
| Vulkan loader / headers | Apache-2.0 / MIT | system component on Windows and Linux |

> This fork **does not use FFmpeg**, which avoids the GPL/LGPL questions that
> arise when bundling an FFmpeg build. Audio decoding uses `symphonia` (MPL-2.0).
> If FFmpeg is ever added, it must be an LGPL build, dynamically linked.
