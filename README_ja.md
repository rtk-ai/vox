<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  5つのバックエンドとAIアシスタント向けMCPサーバーを備えたクロスプラットフォームTTS CLIツール。
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
# クイックインストール (macOS / Linux / WSL)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# ソースから
cargo install --path .

# GPU アクセラレーション
cargo install --path . --features metal  # macOS Apple Silicon
cargo install --path . --features cuda   # Linux NVIDIA
```

## クイックスタート

```bash
vox "Hello, world."                     # デフォルトバックエンドで読み上げ
vox -b voxtream "Zero-shot TTS."        # VoXtream2（最速）
vox -b kokoro -l ja "こんにちは"         # Kokoro 日本語
vox --volume 2.0 "もっと大きく！"       # 2倍音量（範囲：0.0–5.0）
echo "パイプテキスト" | vox              # 標準入力から読み取り
vox setup                               # インタラクティブ設定（TUI）
```

## AIアシスタント統合

1つのコマンドで**14のAIツール**を設定（Claude Code、Cursor、VS Code、Zed、Codex、Gemini、Amazon Qなど）：

```bash
vox init                # MCPサーバー（デフォルト）— 全ツール
vox init -m cli         # CLAUDE.md + Stopフック
vox init -m all         # 全モード
```

## ボイスクローニング

```bash
vox clone add myvoice --audio ~/voice.wav --text "書き起こし"
vox clone record myvoice --duration 10
vox -v myvoice "あなたの声で話します。"
```

## デーモン（モデル常駐）

```bash
vox daemon start        # モデルをメモリに保持
vox daemon status       # ロード済みバックエンドを表示
vox daemon stop         # 停止
```

## ドキュメント

| ドキュメント | 説明 |
|-------------|------|
| [アーキテクチャ](docs/ARCHITECTURE.md) | 技術アーキテクチャ、バックエンド、DBスキーマ、MCPプロトコル |
| [機能](docs/FEATURES.md) | 全コマンドと機能のドキュメント |
| [ガイド](docs/GUIDE.md) | インストール、クイックスタート、トラブルシューティング |

## ライセンス

[Apache-2.0](LICENSE)
