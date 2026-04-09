<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  CLI TTS multiplataforma con cinco backends y servidor MCP para asistentes de IA.
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

## Instalacion

```bash
# Instalacion rapida (macOS / Linux / WSL)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# Desde el codigo fuente
cargo install --path .

# Con aceleracion GPU
cargo install --path . --features metal  # macOS Apple Silicon
cargo install --path . --features cuda   # Linux NVIDIA
```

## Inicio rapido

```bash
vox "Hola, mundo."                      # Hablar con el backend por defecto
vox -b voxtream "Zero-shot TTS."        # VoXtream2 (el mas rapido)
vox -b kokoro -l es "Hola mundo"        # Kokoro en espanol
vox --volume 2.0 "¡Mas fuerte!"        # Volumen 2x (rango: 0.0–5.0)
echo "Texto pipe" | vox                 # Leer desde stdin
vox setup                               # Configuracion interactiva (TUI)
```

## Integracion con IA

Un comando configura **14 herramientas de IA** (Claude Code, Cursor, VS Code, Zed, Codex, Gemini, Amazon Q, etc.):

```bash
vox init                # Servidor MCP (por defecto) — todas las herramientas
vox init -m cli         # CLAUDE.md + hook Stop
vox init -m all         # Todos los modos
```

## Clonacion de voz

```bash
vox clone add mivoz --audio ~/voz.wav --text "Transcripcion"
vox clone record mivoz --duration 10
vox -v mivoz "Esto habla con tu voz."
```

## Daemon (modelos en memoria)

```bash
vox daemon start        # Mantener modelos en memoria
vox daemon status       # Ver backends cargados
vox daemon stop         # Detener
```

## Documentacion

| Documento | Descripcion |
|-----------|-------------|
| [Arquitectura](docs/ARCHITECTURE.md) | Arquitectura tecnica, backends, esquema DB, protocolo MCP |
| [Funcionalidades](docs/FEATURES.md) | Documentacion de todos los comandos |
| [Guia](docs/GUIDE.md) | Instalacion, inicio rapido, solucion de problemas |

## Licencia

[Apache-2.0](LICENSE)
