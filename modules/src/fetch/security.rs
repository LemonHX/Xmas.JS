use hyper::Uri;
use rsquickjs::{Ctx, Error, Exception, Result};

use crate::permissions;

pub fn ensure_url_access(ctx: &Ctx<'_>, uri: &Uri) -> Result<()> {
    let permissions = ctx.userdata::<permissions::Permissions>().unwrap();
    match &permissions.net {
        permissions::BlackOrWhiteList::BlackList(items) => {
            if url_match(
                &items
                    .iter()
                    .map(|e| Uri::try_from(e))
                    .filter_map(|ruri| match ruri {
                        Ok(uri) => Some(uri),
                        Err(_) => None,
                    })
                    .collect::<Vec<Uri>>(),
                uri,
            ) {
                return Err(url_restricted_error(ctx, "URL denied", uri));
            }
        }
        permissions::BlackOrWhiteList::WhiteList(items) => {
            if !url_match(
                &items
                    .iter()
                    .map(|e| Uri::try_from(e))
                    .filter_map(|ruri| match ruri {
                        Ok(uri) => Some(uri),
                        Err(_) => None,
                    })
                    .collect::<Vec<Uri>>(),
                uri,
            ) {
                return Err(url_restricted_error(ctx, "URL not allowed", uri));
            }
        }
    }
    // if let Some(allow_list) = HTTP_ALLOW_LIST.get() {
    //     if !url_match(allow_list, uri) {
    //         return Err(url_restricted_error(ctx, "URL not allowed", uri));
    //     }
    // }

    // if let Some(deny_list) = HTTP_DENY_LIST.get() {
    //     if url_match(deny_list, uri) {
    //         return Err(url_restricted_error(ctx, "URL denied", uri));
    //     }
    // }

    Ok(())
}

fn url_restricted_error(ctx: &Ctx<'_>, message: &str, uri: &Uri) -> Error {
    let uri_host = uri.host().unwrap_or_default();
    let mut message_string = String::with_capacity(message.len() + 100);
    message_string.push_str(message);
    message_string.push_str(": ");
    message_string.push_str(uri_host);
    if let Some(port) = uri.port_u16() {
        message_string.push(':');
        message_string.push_str(itoa::Buffer::new().format(port))
    }

    Exception::throw_message(ctx, &message_string)
}

fn url_match(list: &[Uri], uri: &Uri) -> bool {
    let host = uri.host().unwrap_or_default();
    let port = uri.port_u16().unwrap_or(80);
    list.iter().any(|entry| {
        host.ends_with(entry.host().unwrap_or_default()) && entry.port_u16().unwrap_or(80) == port
    })
}
