use std::collections::HashSet;

use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub struct UniqueIdVendor {
    map: IndexMap<String, String>,
    avoid: HashSet<String>,
}

impl UniqueIdVendor {
    pub fn new() -> UniqueIdVendor {
        UniqueIdVendor {
            map: IndexMap::new(),
            avoid: HashSet::new(),
        }
    }

    // If desired_name is taken will add _2, _3, etc.
    pub fn get_uniq_id(&mut self, desired_name: &str) -> String {
        let mut name = desired_name.to_string();
        let mut i = 2;
        while self.avoid.contains(&name) || self.map.contains_key(&name) {
            name = format!("{}_{}", desired_name, i);
            i += 1;
        }
        self.map.insert(name.clone(), name.clone());

        name
    }

    pub fn avoid(&mut self, name: &str) {
        self.avoid.insert(name.to_string());
    }

    pub fn dont_avoid(&mut self, name: &str) {
        self.avoid.remove(name);
    }

    // Provided desired names ("n", "na", "nam") find available one
    // If none are available, will add _2, _3 to last option.
    pub fn get_one_of_uniq_id(&mut self, desired_names: Vec<&str>) -> String {
        for name in &desired_names {
            if self.avoid.contains(&name.to_string()) {
                continue;
            }
            if !self.map.contains_key(*name) {
                self.map.insert(name.to_string(), name.to_string());
                return name.to_string();
            }
        }

        let last_option = desired_names.last().unwrap();
        self.get_uniq_id(last_option)
    }

    pub fn all_prefixes(name: &str) -> Vec<&str> {
        (1..name.len()).into_iter().map(|i| &name[..i]).collect()
    }

    // Check for identical keys in either the avoid set or map between two vendors
    pub fn has_conflict(&self, other: &UniqueIdVendor) -> bool {
        // Check if any key in self.avoid is in other.avoid or other.map
        for key in &self.avoid {
            if other.avoid.contains(key) || other.map.contains_key(key) {
                return true;
            }
        }

        // Check if any key in self.map is in other.avoid or other.map
        for key in self.map.keys() {
            if other.avoid.contains(key) || other.map.contains_key(key) {
                return true;
            }
        }

        false
    }

    pub fn merge(&mut self, other: &mut UniqueIdVendor) {
        for (key, value) in &other.map {
            self.map.insert(key.clone(), value.clone());
        }
        for key in &other.avoid {
            self.avoid.insert(key.clone());
        }
    }
}

// Testing the new method
#[cfg(test)]
mod conflict_tests {
    use super::*;

    #[test]
    fn test_has_conflict() {
        let mut vendor1 = UniqueIdVendor::new();
        let mut vendor2 = UniqueIdVendor::new();

        vendor1.avoid("conflict");
        vendor2
            .map
            .insert("conflict".to_string(), "value".to_string());

        assert!(vendor1.has_conflict(&vendor2));
    }

    #[test]
    fn test_no_conflict() {
        let mut vendor1 = UniqueIdVendor::new();
        let mut vendor2 = UniqueIdVendor::new();

        vendor1.avoid("unique1");
        vendor2
            .map
            .insert("unique2".to_string(), "value".to_string());

        assert!(!vendor1.has_conflict(&vendor2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_id() {
        let mut vendor = UniqueIdVendor::new();

        assert_eq!(vendor.get_uniq_id("name"), "name");
        assert_eq!(vendor.get_uniq_id("name"), "name_2");
        assert_eq!(vendor.get_uniq_id("name"), "name_3");
        assert_eq!(vendor.get_uniq_id("surname"), "surname");
    }

    #[test]
    fn test_prefixes() {
        assert_eq!(UniqueIdVendor::all_prefixes("name"), vec!["n", "na", "nam"]);
    }

    #[test]
    fn test_avoid() {
        let mut vendor = UniqueIdVendor::new();
        vendor.avoid("name");

        assert_eq!(vendor.get_uniq_id("name"), "name_2");
    }

    #[test]
    fn test_one_of_uniq_id() {
        let mut vendor = UniqueIdVendor::new();
        vendor.avoid("nam");

        assert_eq!(
            vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
            "n"
        );
        assert_eq!(
            vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
            "na"
        );
        assert_eq!(
            vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
            "nam_2"
        );
        assert_eq!(
            vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
            "nam_3"
        );
    }
}
