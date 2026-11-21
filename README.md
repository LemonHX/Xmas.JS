<div align="center">

![XMASJS LOGO](./Xmas.JS.svg)
> **above logo does not represent any religious affiliation** , it is simply a stylized representation of "Xmas" as in "ECMAScript".

# Xmas.JS

**A Modern System Scripting Runtime for the JavaScript Era**

[![License: Apache-2.0 OR GPL-3.0](https://img.shields.io/badge/License-Apache%202.0%20OR%20GPL%203.0-blue.svg)](LICENSE)
[![WinterTC Compatible](https://img.shields.io/badge/WinterTC-Compatible-green.svg)](https://wintertc.org/)
[![Built with QuickJS](https://img.shields.io/badge/Built%20with-QuickJS-orange.svg)](https://bellard.org/quickjs/)

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
- ✅ **Async I/O** - smol drives the event loop; promises, timers and I/O run on it
- ✅ **Rust Extensions** - Native module system for system-level access
- ✅ **Interactive REPL** - Built-in read-eval-print loop for rapid prototyping

### In Development
- 🚧 **Package Manager** - Built-in dependency management (no need for npm/pnpm)
- 🚧 **Cross-Platform Shell** - Execute package.json scripts anywhere
- 🚧 **Built-in Toolchain** - TypeScript and TSX compilation, powered by [OXC](https://oxc-project.github.io/)
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

## 🚀 Quick Start

### Running Scripts

```bash
# Run a TypeScript/JavaScript file directly
xmas script.ts
xmas app.js

# Run with verbose output
xmas -v script.ts

# Run in a specific directory
xmas --cwd ./my-project script.ts
```

### Interactive REPL

```bash
# Start the REPL (default when no arguments)
xmas

# Or explicitly
xmas repl
```

The REPL supports TypeScript, TSX, and JSX syntax with syntax highlighting:

```
🎄 >> const x: number = 42
🎄 >> console.log(`Hello, ${x}!`)
Hello, 42!
🎄 >> const element = <div>Hello World</div>
```

### Package Management

Xmas.JS includes a built-in package manager (no need for npm/pnpm/yarn):

```bash
# Install dependencies from package.json
xmas install
xmas i              # shorthand

# Add a package
xmas add lodash
xmas add -D vitest  # add as devDependency
xmas add --pin zod  # pin to exact version

# Remove a package
xmas remove lodash
xmas rm lodash      # shorthand

# Run a script from package.json
xmas run dev
xmas run build --watch src/

# Update lockfile
xmas update

# Upgrade packages to latest versions
xmas upgrade
xmas upgrade --pin  # pin upgraded versions

# Clean node_modules and cache
xmas clean

# Execute a command
xmas exec tsc --version

# Find why a package is installed
xmas why lodash
xmas why lodash 4.17.21

# Create new project from starter kit
xmas create vite

# Download and execute a package (like npx)
xmas x create-react-app my-app
```

### CLI Reference

```
xmas [OPTIONS] [SCRIPT]... [COMMAND]

Commands:
  install (i)     Install packages defined in package.json
  add (a)         Add package to package.json
  remove (rm)     Remove package from package.json
  run             Run a script defined in package.json
  update          Prepare and save a newly planned lockfile
  upgrade         Update packages to the latest available version
  clean           Clean node_modules and cache
  exec            Execute a command (not a script)
  why             Find all uses of a given package
  create          Create new project from a starter kit
  x               Download and execute a package (like npx)

Options:
  -v, --verbose       Print verbose logs
      --cwd <PATH>    Run in a custom working directory
  -h, --help          Print help
  -V, --version       Print version
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
- [x] Async I/O on smol
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

## ❓ FAQ

### Is TypeScript support first-class or is there transpilation?

**TypeScript/TSX/JSX is transpiled to JavaScript before execution.** Xmas.JS uses [OXC](https://oxc-project.github.io/) (a high-performance Rust-based toolchain) to transform TypeScript to JavaScript on-the-fly. This means:

- ✅ **No separate build step required** - Just run `.ts`, `.tsx`, or `.jsx` files directly
- ✅ **Instant transpilation** - OXC is extremely fast (~100x faster than tsc)
- ✅ **Full TypeScript syntax support** - Including decorators, JSX, and modern features
- ⚠️ **No type checking at runtime** - Types are stripped during transpilation (same as `tsc --transpileOnly`)

```bash
# Run TypeScript directly - transpilation happens automatically
xmas script.ts

# REPL also supports TypeScript/TSX
🎄 >> const x: number = 42
🎄 >> const element = <div>Hello</div>
```

For type checking, use your IDE's TypeScript integration or run `tsc --noEmit` separately.

### How is Xmas.JS different from Node.js/Deno/Bun?

| Aspect                       | Xmas.JS                                 | Node.js/Deno/Bun             |
| ---------------------------- | --------------------------------------- | ---------------------------- |
| **Primary Use Case**         | System scripting, CLI tools, serverless | Web servers, full-stack apps |
| **JS Engine**                | QuickJS (interpreter)                   | V8/JSC (JIT compiler)        |
| **Startup Time**             | ~5-15ms                                 | ~50-200ms                    |
| **Memory Footprint**         | ~3-8MB                                  | ~25-60MB                     |
| **Long-running Performance** | Slower (no JIT)                         | Faster (JIT optimized)       |
| **Binary Size**              | ~5MB                                    | ~50-100MB                    |

**Choose Xmas.JS when:** You need fast startup, low memory, or are replacing Python/Bash scripts.

**Choose Node.js/Deno/Bun when:** You're building web servers or compute-intensive applications.

### Can I use npm packages with Xmas.JS?

**Yes, with some caveats:**

- ✅ **Pure JavaScript/TypeScript packages** - Work out of the box
- ✅ **Most polyfilled packages** - If they don't rely on Node.js internals
- ⚠️ **Native addons (.node files)** - Not supported (C++ extensions compiled for Node.js)
- ⚠️ **Node.js-specific APIs** - Some may not be implemented yet (check our compatibility table)

```bash
# Use the built-in package manager
xmas install lodash
xmas install zod
```

We're continuously improving Node.js API compatibility. Check [TODO.md](TODO.md) for current status.

### Is Xmas.JS production-ready?

**Not yet.** Xmas.JS is in active development (pre-1.0). We recommend:

- ✅ **Use for:** Personal scripts, internal tools, prototyping, learning
- ⚠️ **Evaluate for:** Non-critical production workloads with thorough testing
- ❌ **Avoid for:** Mission-critical production systems (for now)

We're targeting a stable 1.0 release in Q1 2026.

### Why QuickJS instead of V8?

**QuickJS was chosen deliberately for our use case:**

| Feature           | QuickJS   | V8      |
| ----------------- | --------- | ------- |
| Startup time      | ~5ms      | ~100ms  |
| Memory overhead   | ~3MB      | ~30MB   |
| Binary size       | ~1MB      | ~30MB   |
| JIT compilation   | ❌ No      | ✅ Yes   |
| Long-running perf | Slower    | Faster  |
| Embedding ease    | Very easy | Complex |

For **short-lived scripts** (CLI tools, serverless, automation), the fast startup and low memory of QuickJS outweighs V8's JIT benefits. V8's JIT only helps after the code has run long enough to be optimized.

### Does Xmas.JS support WebAssembly?

**Planned for Q1 2026.** QuickJS has experimental WASM support, and we're working on integrating it with proper WinterTC-compatible APIs.

### Can I embed Xmas.JS in my Rust application?

**Yes!** Xmas.JS is designed to be embeddable:

```rust
use xmas_js_modules::prelude::*;

// Create a runtime
let runtime = AsyncRuntime::new()?;
let context = AsyncContext::full(&runtime).await?;

// Run JavaScript
context.with(|ctx| {
    ctx.eval("console.log('Hello from embedded JS!')")?;
    Ok(())
}).await?;
```

See our [embedding guide](docs/embedding.md) for detailed instructions.

### How does Xmas.JS handle async/await?

Xmas.JS runs its event loop on **smol**, providing:

- ✅ Full `async/await` support in JavaScript
- ✅ `Promise` API compatible with web standards
- ✅ Concurrent I/O operations (file system, network, timers)
- ✅ Top-level await in ES modules

```javascript
// Async operations work just like in Node.js/Deno
const response = await fetch('https://api.example.com/data');
const data = await response.json();

// Parallel operations
const [file1, file2] = await Promise.all([
  fs.promises.readFile('a.txt'),
  fs.promises.readFile('b.txt')
]);
```

### What's the relationship with WinterTC?

[WinterTC](https://wintertc.org/) (Web-interoperable Runtimes Community Group) defines standard APIs for non-browser JavaScript runtimes. Xmas.JS aims to be **WinterTC-compatible**, meaning:

- ✅ Standard `fetch()`, `Request`, `Response` APIs
- ✅ Web Crypto API (`crypto.subtle`)
- ✅ Web Streams API
- ✅ `URL`, `URLSearchParams`, `TextEncoder`, `TextDecoder`
- ✅ `console`, `setTimeout`, `setInterval`

This ensures code portability between Xmas.JS, Deno, Cloudflare Workers, and other WinterTC-compatible runtimes.

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
- **[quickjs-oxide](https://github.com/0xfe-project/quickjs-oxide)** - Rust bindings for QuickJS, embedded as a package (rquickjs descendant)
- **[LLRT](https://github.com/awslabs/llrt)** - Inspiration and code for AWS Lambda optimization
- **[smol](https://github.com/smol-rs/smol)** - The async runtime, everywhere in the workspace. Nothing depends on tokio.
- **[Cotton](https://github.com/danielhuang/cotton)** - Package manager forked for our needs

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
