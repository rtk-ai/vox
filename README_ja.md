<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  AIコーディングエージェント向けのローカル音声ツール。音声合成と音声認識を1つのRustバイナリで提供し、複数のTTSバックエンド、Whisper、MCPサーバーを備えています。
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README_fr.md">Fran&ccedil;ais</a> &bull;
  <a href="README_zh.md">中文</a> &bull;
  <a href="README_ja.md">日本語</a> &bull;
  <a href="README_ko.md">한국어</a> &bull;
  <a href="README_es.md">Espa&ntilde;ol</a>
</p>

---

## インストール

```bash
# クイックインストール (macOS Apple Silicon、Linux x86_64 / ARM64、WSL2)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# Homebrew (macOS Apple Silicon、Linux)
brew install rtk-ai/tap/vox
```

GPUビルド（Metal、CUDA）、ソースからのビルド、動作要件については[英語版README](README.md#install)を参照してください。`cargo install vox` は実行しないでください。crates.io ではその名前は別のプロジェクトのものです。

## バックエンド

| バックエンド | 言語 | ボイスクローニング | 利用できる環境 |
|-------------|------|-------------------|---------------|
| `pocket` | 英語 | 可（`HF_TOKEN` が必要） | 全プラットフォーム。英語の場合と言語を指定しない場合のデフォルト |
| `piper` | 11言語（日本語は含まれません） | 不可 | 全プラットフォーム。英語以外の言語のデフォルト |
| `qwen-native` | 日本語を含む10言語 | 可 | 全プラットフォーム。Metal または CUDA ビルドでは GPU を使用 |
| `say` | システムの音声 | 不可 | macOS のみ |
| `kokoro` | — | 不可 | `--features kokoro` を付けてコンパイルしたビルドのみ。配布バイナリでは `Unknown backend: kokoro` と表示されます |

Python はどこにも使われていません。音声認識は Whisper で、全プラットフォームで動作します（`vox hear`）。

日本語について：`vox -l ja` は `piper` を選びますが、コードが指定する日本語音声のダウンロード先は `404` を返すため失敗します（2026-10-05 に確認）。日本語には `-b qwen-native` を使ってください。

## クイックスタート

```bash
vox "Hello, world."                     # デフォルトバックエンド（pocket、英語）
vox -b qwen-native "こんにちは"          # 日本語：qwen-native（初回は約2.5GBをダウンロード）
vox --volume 2.0 "Louder!"              # 2倍音量（範囲：0.0–5.0）
echo "Piped text" | vox                 # 標準入力から読み取り
vox setup                               # インタラクティブ設定（TUI）
```

## AIアシスタント統合

1つのコマンドで**14のAIツール**を設定（Claude Code、Cursor、VS Code、Zed、Codex、Gemini、Amazon Qなど）：

```bash
vox init                # MCPサーバー（デフォルト）— 全ツール
vox init -m cli         # CLAUDE.md + Stopフック
vox init -m all         # 全モード
```

## Claude Code プラグイン：音声ビジュアライザー

vox には Claude Code プラグインが付属しており、vox が話している間、プロンプトの上に実際の音声スペクトラムを表示します。Claude Code（2.1.287 以降）のセッションで次を実行します：

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
/vox-wave                          # プレビューと色の設定
```

詳細は [plugins/vox](plugins/vox/README.md)（英語）を参照してください。

## ボイスクローニング

```bash
vox clone add myvoice --audio ~/voice.wav --text "書き起こし"     # 音声ファイルから
vox clone record myvoice2 --duration 10                          # またはマイクで録音
vox -b qwen-native -l ja -v myvoice "あなたの声で話します。"
```

クローニングには `qwen-native` を使います。`pocket` でクローニングするには `HF_TOKEN` が必要です。

## デーモン（モデル常駐）

```bash
vox daemon start        # モデルをメモリに保持
vox daemon status       # ロード済みバックエンドを表示
vox daemon stop         # 停止
```

デーモンは自動では起動しません。

## ドキュメント

以下のドキュメントはフランス語で書かれています。

| ドキュメント | 説明 |
|-------------|------|
| [アーキテクチャ](docs/ARCHITECTURE.md) | 技術アーキテクチャ、バックエンド、DBスキーマ、MCPプロトコル |
| [機能](docs/FEATURES.md) | 全コマンドと機能のドキュメント |
| [ガイド](docs/GUIDE.md) | インストール、クイックスタート、トラブルシューティング |

## ライセンス

[Apache-2.0](LICENSE)
