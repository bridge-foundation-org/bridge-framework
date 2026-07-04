use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Store {
    namespaces: HashMap<String, HashMap<String, String>>,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, namespace: &str, key: &str, value: String) {
        self.namespaces
            .entry(namespace.to_string())
            .or_default()
            .insert(key.to_string(), value);
    }

    pub fn get(&self, namespace: &str, key: &str) -> Option<&str> {
        self.namespaces
            .get(namespace)
            .and_then(|bucket| bucket.get(key))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn put_and_get_value() {
        let mut store = Store::new();
        store.put("codegen", "hello", "client".to_string());
        assert_eq!(store.get("codegen", "hello"), Some("client"));
    }
}
