use std::sync::{Arc, RwLock};

use rsquickjs::{
    class::{Trace, Tracer},
    JsLifetime,
};

use super::{Emitter, EventList, Events};

#[rsquickjs::class]
#[derive(Clone)]
pub struct EventTarget<'js> {
    pub events: Events<'js>,
}

unsafe impl<'js> JsLifetime<'js> for EventTarget<'js> {
    type Changed<'to> = EventTarget<'to>;
}

impl<'js> Emitter<'js> for EventTarget<'js> {
    fn get_event_list(&self) -> Arc<RwLock<EventList<'js>>> {
        self.events.clone()
    }
}

impl<'js> Trace<'js> for EventTarget<'js> {
    fn trace<'a>(&self, tracer: Tracer<'a, 'js>) {
        self.trace_event_emitter(tracer);
    }
}

#[rsquickjs::methods]
impl<'js> EventTarget<'js> {
    #[qjs(constructor)]
    pub fn new() -> Self {
        Self {
            #[allow(clippy::arc_with_non_send_sync)]
            events: Arc::new(RwLock::new(Vec::new())),
        }
    }
}