use crate::utils::module::{export_default, ModuleInfo};
use rsquickjs::{
    module::{Declarations, Exports, ModuleDef},
    prelude::Func,
    Ctx, Result,
};

pub mod lookup;

pub struct DnsModule;

impl ModuleDef for DnsModule {
    fn declare(declare: &Declarations) -> Result<()> {
        declare.declare("lookup")?;

        declare.declare("default")?;
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        export_default(ctx, exports, |default| {
            default.set("lookup", Func::from(lookup::lookup))?;
            Ok(())
        })?;

        Ok(())
    }
}

impl From<DnsModule> for ModuleInfo<DnsModule> {
    fn from(val: DnsModule) -> Self {
        ModuleInfo {
            name: "dns",
            module: val,
        }
    }
}
