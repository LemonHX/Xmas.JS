#[derive(rquickjs::class::Trace, rquickjs::JsLifetime)]
/// Struct representing permissions for filesystem, network, and environment access.
/// **WARNING**: by default, no permissions are granted (all whitelists are empty).
pub struct Permissions {
    pub fs: BlackOrWhiteList,
    pub net: BlackOrWhiteList,
    pub env: BlackOrWhiteList,
    pub stdio: bool,
}

#[derive(Debug, PartialEq, Clone, Hash, Eq, rquickjs::class::Trace, rquickjs::JsLifetime)]
pub enum BlackOrWhiteList {
    BlackList(Vec<String>),
    WhiteList(Vec<String>),
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            fs: BlackOrWhiteList::WhiteList(vec![]),
            net: BlackOrWhiteList::WhiteList(vec![]),
            env: BlackOrWhiteList::WhiteList(vec![]),
            stdio: false,
        }
    }
}

impl Permissions {
    pub fn allow_all() -> Self {
        Self {
            fs: BlackOrWhiteList::BlackList(vec![]),
            net: BlackOrWhiteList::BlackList(vec![]),
            env: BlackOrWhiteList::BlackList(vec![]),
            stdio: true,
        }
    }
}

pub fn init(ctx: rquickjs::Ctx<'_>, permissions: Permissions) -> rquickjs::Result<()> {
    ctx.store_userdata(permissions)?;
    Ok(())
}