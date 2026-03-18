# Changelog

## [0.9.0](https://github.com/rtk-ai/vox/compare/v0.8.0...v0.9.0) (2026-03-18)


### Features

* add lazy daemon mode and streaming voxtream-server integration ([#36](https://github.com/rtk-ai/vox/issues/36)) ([47a7609](https://github.com/rtk-ai/vox/commit/47a7609ab22e9f6c716de312e0f27181920dc3e8)), closes [#30](https://github.com/rtk-ai/vox/issues/30)


### Bug Fixes

* auto-fix for issue [#33](https://github.com/rtk-ai/vox/issues/33) [wshm] ([#34](https://github.com/rtk-ai/vox/issues/34)) ([bcf0e25](https://github.com/rtk-ai/vox/commit/bcf0e25425dc9f7c58200f300501aad914adbf79))

## [0.8.0](https://github.com/rtk-ai/vox/compare/v0.7.0...v0.8.0) (2026-03-18)


### Features

* add VoXtream2 backend and interactive TUI setup ([#31](https://github.com/rtk-ai/vox/issues/31)) ([5c7799c](https://github.com/rtk-ai/vox/commit/5c7799cd3cc5673b467e1caf5c09684ce3287c97))

## [0.7.0](https://github.com/rtk-ai/vox/compare/v0.6.0...v0.7.0) (2026-03-16)


### Features

* add UX, performance, and security test suites ([#24](https://github.com/rtk-ai/vox/issues/24)) ([c421483](https://github.com/rtk-ai/vox/commit/c421483db9382a0590fd8d63471661c51bb34772))

## [0.6.0](https://github.com/rtk-ai/vox/compare/v0.5.0...v0.6.0) (2026-03-05)


### Features

* enrich vox stats + fix broken STT tests ([22923ca](https://github.com/rtk-ai/vox/commit/22923ca7144b9a08dabec223c010dcdad34297a0))


### Bug Fixes

* default to say backend on macOS, kokoro on other platforms ([#23](https://github.com/rtk-ai/vox/issues/23)) ([c624028](https://github.com/rtk-ai/vox/commit/c624028eabc64030710477be04dd95c15678bbba))
* update README license from MIT to source-available ([#22](https://github.com/rtk-ai/vox/issues/22)) ([080de05](https://github.com/rtk-ai/vox/commit/080de055b525bf50942665fb99be779d13ae7609))

## [0.5.0](https://github.com/rtk-ai/vox/compare/v0.4.0...v0.5.0) (2026-03-05)


### Features

* add 6 AI tools to vox init (Gemini, Amazon Q, Cline, Roo Code, Kilo Code, Amp) ([65c73a2](https://github.com/rtk-ai/vox/commit/65c73a23a6f1bc3795cc04f5a7d0badff99d6d87))
* add Kokoro TTS backend, universal MCP init, source-available license ([413ff83](https://github.com/rtk-ai/vox/commit/413ff83e2136f28c9b172f2239b0abc9d69a913b))


### Bug Fixes

* remove Stop hook (terminé) from project settings ([085725a](https://github.com/rtk-ai/vox/commit/085725a77c40cfbfdcf98f3fc203283ce1220464))

## [0.4.0](https://github.com/rtk-ai/vox/compare/v0.3.1...v0.4.0) (2026-02-12)


### Features

* add sound pack system (peon-ping compatible) ([#18](https://github.com/rtk-ai/vox/issues/18)) ([b048487](https://github.com/rtk-ai/vox/commit/b048487a7f897b4bee0b5f4fb1ffc484d36873d1))

## [0.3.1](https://github.com/rtk-ai/vox/compare/v0.3.0...v0.3.1) (2026-02-10)


### Bug Fixes

* default init mode to mcp instead of all ([0b414f5](https://github.com/rtk-ai/vox/commit/0b414f580a2e35db7541ed2e678363b1745d95ec))
* default init mode to mcp instead of all ([7835952](https://github.com/rtk-ai/vox/commit/78359528923173e5b99833f3b2986005c2f43632))
* pass secrets to release workflow for Homebrew update ([c025d9d](https://github.com/rtk-ai/vox/commit/c025d9d7c5bcb4f53f716f5f212326dc6078ee55))

## [0.3.0](https://github.com/rtk-ai/vox/compare/v0.2.1...v0.3.0) (2026-02-10)


### Features

* add MCP server and cross-platform improvements ([995f3d4](https://github.com/rtk-ai/vox/commit/995f3d45e7f3b30441522a530f0a0c5421b07c07))


### Bug Fixes

* cross-platform home detection and CI workflow_dispatch ([7db9d03](https://github.com/rtk-ai/vox/commit/7db9d03cb00992a3ac21a159d4c1a4f0a4f5b748))

## [0.2.1](https://github.com/rtk-ai/vox/compare/v0.2.0...v0.2.1) (2026-02-06)


### Bug Fixes

* release workflow binary upload when called via workflow_call ([#10](https://github.com/rtk-ai/vox/issues/10)) ([a215c52](https://github.com/rtk-ai/vox/commit/a215c52fc8062b804a3682fdda53a1599573ff07))

## [0.2.0](https://github.com/rtk-ai/vox/compare/v0.1.0...v0.2.0) (2026-02-06)


### Features

* cross-platform support for Linux CUDA and Windows ([#8](https://github.com/rtk-ai/vox/issues/8)) ([1ec501f](https://github.com/rtk-ai/vox/commit/1ec501f55a3722416d7cf21899da536bd52010d2))

## [0.1.0](https://github.com/rtk-ai/vox/compare/v0.0.2...v0.1.0) (2026-02-06)


### Features

* add qwen-native backend using qwen3-tts-rs for pure Rust TTS inference ([db2296c](https://github.com/rtk-ai/vox/commit/db2296c4f8e581edecd677caee5385a20fcbf7eb))
* parallelize CI and fix release workflow for macOS ([170a75f](https://github.com/rtk-ai/vox/commit/170a75fe1037f601df0a3c2633b9137494813ce2))
* smart sentence merging and configurable model for faster TTS ([f73501c](https://github.com/rtk-ai/vox/commit/f73501c52532ca27a49825a4a86c6103dacb72f6))

## [0.0.2](https://github.com/rtk-ai/vox/compare/v0.0.1...v0.0.2) (2026-02-02)


### Bug Fixes

* add brew install python3 for Qwen requirements ([de7c23d](https://github.com/rtk-ai/vox/commit/de7c23decd69b4adff605be7f53f476183380e44))
* apply cargo fmt formatting ([622ae84](https://github.com/rtk-ai/vox/commit/622ae844f6a969a3ebf04cfe8d774c26c64dd03a))
* document Qwen backend requirements in README ([055450a](https://github.com/rtk-ai/vox/commit/055450a7e41c21d9052f5758651279c7ae04ddef))
* format with rustfmt 1.93 to match CI ([b7c0459](https://github.com/rtk-ai/vox/commit/b7c045928e792828f254977f5dedba1fff15744d))
* resolve all clippy warnings (collapsible_if, from_str rename) ([b1648ed](https://github.com/rtk-ai/vox/commit/b1648edbf63a45fb8397f03c4adfe57861c77e77))
