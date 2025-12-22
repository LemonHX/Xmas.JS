use crate::utils::module::{export_default, ModuleInfo};
use rsquickjs::{
    module::{Declarations, Exports, ModuleDef},
    Class, Ctx, Result,
};

pub mod agent;
pub mod client;
pub mod dns_cache;

pub struct HttpsModule;

impl ModuleDef for HttpsModule {
    fn declare(declare: &Declarations) -> Result<()> {
        declare.declare(stringify!(Agent))?;
        declare.declare("default")?;
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        export_default(ctx, exports, |default| {
            Class::<agent::Agent>::define(default)?;

            Ok(())
        })
    }
}

impl From<HttpsModule> for ModuleInfo<HttpsModule> {
    fn from(val: HttpsModule) -> Self {
        ModuleInfo {
            name: "https",
            module: val,
        }
    }
}
