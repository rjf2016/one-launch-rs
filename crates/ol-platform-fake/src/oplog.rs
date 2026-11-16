use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Map, Value};

// Everything the fake machine was asked to do, in the order it was asked. A test reads it to show what a switch touched and what it left alone.
#[derive(Clone, Default)]
pub struct OpLog(Arc<Mutex<Vec<Value>>>);

impl OpLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, op: &str, fields: impl IntoIterator<Item = (&'static str, Value)>) {
        let mut event = Map::new();
        event.insert("k".to_owned(), "plat".into());
        event.insert("op".to_owned(), op.into());
        for (name, value) in fields {
            event.insert(name.to_owned(), value);
        }
        self.push(Value::Object(event));
    }

    pub fn push(&self, event: Value) {
        self.events().push(event);
    }

    pub fn snapshot(&self) -> Vec<Value> {
        self.events().clone()
    }

    fn events(&self) -> MutexGuard<'_, Vec<Value>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
