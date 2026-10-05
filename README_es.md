<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  Voz local para asistentes de IA: sintesis y reconocimiento de voz en un solo binario Rust, con varios backends TTS, Whisper y un servidor MCP.
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
# Instalacion rapida (macOS Apple Silicon, Linux x86_64 y ARM64, WSL2)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# Homebrew (macOS Apple Silicon, Linux)
brew install rtk-ai/tap/vox
```

Para los builds con GPU (Metal, CUDA), la compilacion desde el codigo fuente y
los requisitos, consulta el [README en ingles](README.md#install). No ejecutes
`cargo install vox`: en crates.io ese nombre pertenece a otro proyecto.

## Backends

| Backend | Idiomas | Clonacion de voz | Disponibilidad |
|---------|---------|------------------|----------------|
| `pocket` | ingles | si, con `HF_TOKEN` | Todas las plataformas. Por defecto para el ingles y cuando no se indica idioma |
| `piper` | 11 idiomas, entre ellos el espanol | no | Todas las plataformas. Por defecto para los demas idiomas |
| `qwen-native` | 10 idiomas | si | Todas las plataformas. Usa la GPU en un build Metal o CUDA |
| `say` | voces del sistema | no | Solo macOS |
| `kokoro` | — | no | Solo en un build compilado con `--features kokoro`. Los binarios publicados responden `Unknown backend: kokoro` |

No hay Python en ninguna parte. El reconocimiento de voz usa Whisper y funciona
en todas las plataformas (`vox hear`).

## Inicio rapido

```bash
vox "Hello, world."                     # Backend por defecto (pocket, ingles)
vox -l es "Hola, mundo."                # Espanol: backend piper
vox -l es --volume 2.0 "¡Mas fuerte!"   # Volumen 2x (rango: 0.0–5.0)
echo "Texto pipe" | vox -l es           # Leer desde stdin
vox setup                               # Configuracion interactiva (TUI)
```

## Integracion con IA

Un comando configura **14 herramientas de IA** (Claude Code, Cursor, VS Code, Zed, Codex, Gemini, Amazon Q, etc.):

```bash
vox init                # Servidor MCP (por defecto) — todas las herramientas
vox init -m cli         # CLAUDE.md + hook Stop
vox init -m all         # Todos los modos
```

## Plugin de Claude Code: visualizador de voz

vox incluye un plugin de Claude Code que muestra el espectro real de la voz
encima del prompt mientras vox habla. En una sesion de Claude Code (2.1.287 o
posterior):

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
/vox-wave                          # vista previa y colores
```

Detalles en [plugins/vox](plugins/vox/README.md) (en ingles).

## Clonacion de voz

```bash
vox clone add mivoz --audio ~/voz.wav --text "Transcripcion"    # desde un archivo
vox clone record otravoz --duration 10                          # o grabando con el microfono
vox -b qwen-native -l es -v mivoz "Esto habla con tu voz."
```

La clonacion usa `qwen-native`. Con `pocket` hace falta `HF_TOKEN`.

## Daemon (modelos en memoria)

```bash
vox daemon start        # Mantener modelos en memoria
vox daemon status       # Ver backends cargados
vox daemon stop         # Detener
```

El daemon no se inicia automaticamente.

## Documentacion

Estos documentos estan en frances.

| Documento | Descripcion |
|-----------|-------------|
| [Arquitectura](docs/ARCHITECTURE.md) | Arquitectura tecnica, backends, esquema DB, protocolo MCP |
| [Funcionalidades](docs/FEATURES.md) | Documentacion de todos los comandos |
| [Guia](docs/GUIDE.md) | Instalacion, inicio rapido, solucion de problemas |

## Licencia

[Apache-2.0](LICENSE)
