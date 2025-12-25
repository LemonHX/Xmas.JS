<div align="center">

![XMASJS LOGO](./Xmas.JS.svg)
> **above logo does not represent any religious affiliation** , it is simply a stylized representation of "Xmas" as in "ECMAScript".

# Xmas.JS

**A Modern System Scripting Runtime for the JavaScript Era**

[![License: Apache-2.0 OR GPL-3.0](https://img.shields.io/badge/License-Apache%202.0%20OR%20GPL%203.0-blue.svg)](LICENSE)
[![WinterTC Compatible](https://img.shields.io/badge/WinterTC-Compatible-green.svg)](https://wintertc.org/)
[![Built with QuickJS](https://img.shields.io/badge/Built%20with-QuickJS-orange.svg)](https://bellard.org/quickjs/)
[![Powered by Tokio](https://img.shields.io/badge/Powered%20by-Tokio-red.svg)](https://tokio.rs/)

[Features](#-features) • [Installation](#-installation) • [Quick Start](#-quick-start) • [Benchmarks](#-benchmarks) • [Documentation](#-documentation) • [Contributing](#-contributing)

</div>

---

## 🎯 What is Xmas.JS?

Xmas.JS is a **lightweight, high-performance JavaScript/TypeScript runtime** designed to replace traditional system scripting languages like **Lua, Perl, and Python** for system administration, automation, and glue code tasks.

Unlike Node.js, Deno, or Bun which target web applications and server-side development, **Xmas.JS is purpose-built for:**

- 🔧 **System scripting and automation** - Replace Bash, PowerShell, Python scripts
- ⚡ **Serverless and edge computing** - Cold start in milliseconds, not seconds
- 🪶 **Embedded scripting** - Minimal memory footprint (<5MB)
- 🔌 **CLI tools and utilities** - Fast startup for command-line applications
- 🧩 **System integration** - Native Rust modules for deep system access

> **Note:** The word "Xmas" is pronounced like "ECMAS" (ECMAScript), not a religious reference. "JavaScript" in this context refers to ECMAScript/TypeScript, not Oracle's JavaScript™ trademark.

---

## 🚀 Why Xmas.JS?

### The Problem with Existing Runtimes

**QuickJS does not use any sort of JIT compilation**, making it ideal for fast startup and low memory usage, but less suited for long-running web servers.

Modern JavaScript runtimes like Node.js, Deno, and Bun are excellent for **web servers and applications**, but they're **overkill for scripting**:

| Runtime     | Cold Start  | Memory (Idle) | Best Use Case                             |
| ----------- | ----------- | ------------- | ----------------------------------------- |
| **Node.js** | ~100-200ms  | ~30-50MB      | Web servers, long-running apps            |
| **Deno**    | ~150-300ms  | ~40-60MB      | Secure web apps, TypeScript projects      |
| **Bun**     | ~50-100ms   | ~25-35MB      | Fast web development                      |
| **Xmas.JS** | **~5-15ms** | **~3-8MB**    | **System scripts, CLI tools, serverless** |

### The Xmas.JS Difference

```
Traditional System Scripts          Modern System Scripts with Xmas.JS
┌─────────────────────────┐        ┌─────────────────────────┐
│  Python + libraries     │        │  Xmas.JS + TypeScript   │
│  Slow startup           │   →    │  Instant startup        │
│  Heavy dependencies     │        │  Zero dependencies      │
│  Version hell           │        │  Single binary          │
│  Limited async          │        │  Native async/await     │
└─────────────────────────┘        └─────────────────────────┘
```

**Performance Targets:**
- ⚡ **10x faster startup** than Node.js/Deno
- 💰 **2x lower cost** on serverless platforms
- 🪶 **5x smaller memory footprint** than traditional runtimes
- 🔥 **Native performance** via Rust integration

---

## ✨ Features

### Core Capabilities
- ✅ **[WinterTC](https://wintertc.org/) Compatible APIs** - Standard Web APIs (fetch, crypto, streams, etc.)
- ✅ **Modern JavaScript/TypeScript** - Full ES2023+ support including async/await, modules, decorators
- ✅ **Ultra-Fast Startup** - Cold start in ~5-15ms, perfect for CLI and serverless
- ✅ **Minimal Memory Footprint** - Runs comfortably in <5MB RAM
- ✅ **Async I/O** - Powered by Tokio for high-performance concurrent operations
- ✅ **Rust Extensions** - Native module system for system-level access
- ✅ **Interactive REPL** - Built-in read-eval-print loop for rapid prototyping

### In Development
- 🚧 **Package Manager** - Built-in dependency management (no need for npm/pnpm)
- 🚧 **Cross-Platform Shell** - Execute package.json scripts anywhere
- 🚧 **Built-in Toolchain** - Bundler, minifier, TypeScript compiler, linter (powered by [OXC](https://oxc-project.github.io/))
- 🚧 **Bytecode Compilation** - Bundle scripts as bytecode for security and performance
- 🚧 **Full WinterTC Coverage** - Complete Web API compatibility

---

## 📦 Installation

### 🚧 From Binary (Coming soon ❄️)

```bash
# Coming soon - pre-built binaries for major platforms
# Windows
curl -fsSL https://xmas.js.org/install.ps1 | powershell

# macOS / Linux
curl -fsSL https://xmas.js.org/install.sh | sh
```
---

## 📊 Benchmarks

### Startup Time Comparison

```
Python 3.11:     █████████████████████ 45ms
Node.js 20:      ███████████████ 120ms
Deno 1.38:       ██████████████████ 180ms
Bun 1.0:         █████████ 75ms
Xmas.JS:         ██ 12ms ⚡
```

### Memory Usage (Idle)

```
Python 3.11:     ████████████████ 15MB
Node.js 20:      ████████████████████████████ 45MB
Deno 1.38:       ██████████████████████████████ 55MB
Bun 1.0:         ████████████████████ 28MB
Xmas.JS:         ███ 5MB 🪶
```

*Benchmarks performed on Windows 11, AMD Ryzen 9 5900X, 64GB RAM*

---

## 🎯 Use Cases

### Perfect For:
- ✅ **System Administration Scripts** - Replace Python/Perl scripts with modern JavaScript
- ✅ **Build Tools & Automation** - Fast CLI tools that start instantly
- ✅ **Serverless Functions** - Minimal cold start on AWS Lambda, Cloudflare Workers, etc.
- ✅ **IoT & Embedded Devices** - Small memory footprint for resource-constrained environments
- ✅ **Game Scripting** - Embed as a game scripting engine (like Lua)
- ✅ **Configuration Scripts** - Replace complex Bash/PowerShell scripts

### Not Ideal For:
- ❌ **Large Web Applications** - Use Node.js/Deno/Bun instead
- ❌ **Production-Ready Today** - Still in active development

---

## 🗺️ Roadmap

See [TODO.md](TODO.md) for detailed progress.

**2025 Q4**
- [x] Core runtime foundation
- [x] Basic WinterTC APIs
- [x] Async I/O with Tokio
- [x] REPL implementation
- [x] TypeScript support **(repl also supports tsx/jsx)**
- [ ] Bytecode compilation

**2026 Q1**
- [ ] supporting WASM modules
- [ ] Package manager
- [ ] Built-in toolchain (OXC integration)
- [ ] Documentation site
- [ ] 1.0 release candidate

---

## 🤝 Contributing

We welcome contributions! Xmas.JS is in active development and needs help with:

- 🐛 Bug reports and testing
- 📝 Documentation improvements
- ✨ New features and APIs
- 🔧 Performance optimizations
- 🌍 Translations

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## 📄 License

Xmas.JS is dual-licensed under **Apache-2.0 OR GPL-3.0**.

### Use Apache-2.0 if you want to:
- ✅ Use Xmas.JS in proprietary software
- ✅ Contribute to open source projects
- ✅ Build commercial applications
- ✅ Modify the source code

### Use GPL-3.0 if you:
- 🏢 Provide Xmas.JS as a managed service (cloud providers)
- 🔒 Integrate into closed-source infrastructure

This dual-license ensures open collaboration while preventing service provider lock-in.

---

## 🙏 Acknowledgments

Xmas.JS stands on the shoulders of giants:

- **[QuickJS](https://bellard.org/quickjs/)** by Fabrice Bellard - The amazing JavaScript engine
- **[rquickjs](https://github.com/DelSkayn/rquickjs)** - Rust bindings (we maintain a fork)
- **[LLRT](https://github.com/awslabs/llrt)** - Inspiration and code for AWS Lambda optimization
- **[Tokio](https://tokio.rs/)** - Async runtime that powers our I/O

**Inspired by:**
- [Deno](https://deno.land/) - Modern JavaScript runtime design
- [Node.js](https://nodejs.org/) - The JavaScript runtime that started it all
- [txiki.js](https://github.com/saghul/txiki.js) - Lightweight runtime approach

---

## 🌟 Star History

If you find Xmas.JS useful, please consider giving it a star! ✨

[![Star History Chart](https://api.star-history.com/svg?repos=lemonhx/xmas.js&type=date&legend=top-left)](https://www.star-history.com/#lemonhx/xmas.js&type=date&legend=top-left)

---

<div align="center">

Made with ❤️ by the 🍋 LemonHX & 🎄Xmas.JS team

</div>
