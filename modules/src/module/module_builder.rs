use std::collections::HashSet;

use crate::utils::module::ModuleInfo;
use rsquickjs::{module::ModuleDef, Ctx, Result};

use crate::module::module::{loader::ModuleLoader, resolver::ModuleResolver, ModuleNames};

#[derive(Debug, Default)]
pub struct GlobalAttachment {
    names: HashSet<String>,
    functions: Vec<fn(&Ctx<'_>) -> Result<()>>,
}

impl GlobalAttachment {
    pub fn add_function(mut self, init: fn(&Ctx<'_>) -> Result<()>) -> Self {
        self.functions.push(init);
        self
    }

    pub fn add_name<P: Into<String>>(mut self, path: P) -> Self {
        self.names.insert(path.into());
        self
    }

    pub fn attach(self, ctx: &Ctx<'_>) -> Result<()> {
        if !self.names.is_empty() {
            let _ = ctx.store_userdata(ModuleNames::new(self.names));
        }
        for init in self.functions {
            init(ctx)?;
        }
        Ok(())
    }
}

pub struct ModuleBuilder {
    module_resolver: ModuleResolver,
    module_loader: ModuleLoader,
    global_attachment: GlobalAttachment,
}

impl Default for ModuleBuilder {
    fn default() -> Self {
        let mut builder = Self::new();

        builder = builder.with_module(crate::module::module::ModuleModule);
        builder = builder.with_module(crate::async_hooks::AsyncHooksModule);
        builder = builder.with_module(crate::timers::TimersModule);
        #[cfg(feature = "abort")]
        {
            builder = builder.with_global(crate::modules::abort::init);
        }
        #[cfg(feature = "assert")]
        {
            builder = builder.with_module(crate::modules::assert::AssertModule);
        }
        #[cfg(feature = "buffer")]
        {
            builder = builder
                .with_global(crate::modules::buffer::init)
                .with_module(crate::modules::buffer::BufferModule);
        }
        #[cfg(feature = "child-process")]
        {
            builder = builder.with_module(crate::modules::child_process::ChildProcessModule);
        }
        #[cfg(feature = "console")]
        {
            builder = builder.with_module(crate::console::ConsoleModule);
        }
        #[cfg(feature = "crypto")]
        {
            builder = builder.with_module(crate::modules::crypto::CryptoModule);
        }
        #[cfg(feature = "dgram")]
        {
            builder = builder.with_module(crate::modules::dgram::DgramModule);
        }
        #[cfg(feature = "dns")]
        {
            builder = builder.with_module(crate::modules::dns::DnsModule);
        }
        #[cfg(feature = "event")]
        {
            builder = builder.with_module(crate::event::EventsModule);
        }
        #[cfg(feature = "exceptions")]
        {
            builder = builder.with_global(crate::modules::exceptions::init);
        }
        #[cfg(feature = "https")]
        {
            builder = builder.with_module(crate::modules::https::HttpsModule);
        }
        #[cfg(feature = "fetch")]
        {
            builder = builder.with_global(crate::modules::fetch::init);
        }
        #[cfg(feature = "fs")]
        {
            builder = builder
                .with_module(crate::fs::FsPromisesModule)
                .with_module(crate::fs::FsModule);
        }
        #[cfg(feature = "navigator")]
        {
            builder = builder.with_global(crate::modules::navigator::init);
        }
        #[cfg(feature = "net")]
        {
            builder = builder.with_module(crate::modules::net::NetModule);
        }
        #[cfg(feature = "os")]
        {
            builder = builder.with_module(crate::modules::os::OsModule);
        }
        #[cfg(feature = "path")]
        {
            builder = builder.with_module(crate::modules::path::PathModule);
        }
        #[cfg(feature = "perf-hooks")]
        {
            builder = builder
                .with_global(crate::modules::perf_hooks::init)
                .with_module(crate::modules::perf_hooks::PerfHooksModule);
        }
        #[cfg(feature = "process")]
        {
            builder = builder
                .with_global(crate::modules::process::init)
                .with_module(crate::modules::process::ProcessModule);
        }
        #[cfg(feature = "stream-web")]
        {
            builder = builder
                .with_global(crate::modules::stream_web::init)
                .with_module(crate::modules::stream_web::StreamWebModule);
        }
        #[cfg(feature = "string-decoder")]
        {
            builder = builder.with_module(crate::modules::string_decoder::StringDecoderModule);
        }
        #[cfg(feature = "tty")]
        {
            builder = builder.with_module(crate::modules::tty::TtyModule);
        }
        #[cfg(feature = "url")]
        {
            builder = builder
                .with_global(crate::modules::url::init)
                .with_module(crate::modules::url::UrlModule);
        }
        #[cfg(feature = "util")]
        {
            builder = builder
                .with_global(crate::modules::util::init)
                .with_module(crate::modules::util::UtilModule);
        }
        #[cfg(feature = "zlib")]
        {
            builder = builder.with_module(crate::modules::zlib::ZlibModule);
        }

        builder
    }
}

impl ModuleBuilder {
    pub fn new() -> Self {
        Self {
            module_resolver: ModuleResolver::default(),
            module_loader: ModuleLoader::default(),
            global_attachment: GlobalAttachment::default(),
        }
    }

    pub fn with_module<M: ModuleDef, I: Into<ModuleInfo<M>>>(mut self, module: I) -> Self {
        let module_info: ModuleInfo<M> = module.into();

        self.module_resolver = self.module_resolver.add_name(module_info.name);
        self.module_loader = self
            .module_loader
            .with_module(module_info.name, module_info.module);
        self.global_attachment = self.global_attachment.add_name(module_info.name);
        self
    }

    // pub fn with_global(mut self, init: fn(&Ctx<'_>) -> Result<()>) -> Self {
    //     self.global_attachment = self.global_attachment.add_function(init);
    //     self
    // }

    pub fn build(self) -> (ModuleResolver, ModuleLoader, GlobalAttachment) {
        (
            self.module_resolver,
            self.module_loader,
            self.global_attachment,
        )
    }
}
