use std::collections::HashSet;

use anyhow::{anyhow, Result};
use env_logger::fmt::style::Reset;
use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub struct UniqueIdVendor {
    // map: IndexMap<String, String>,
    avoid: HashSet<String>,
}

impl UniqueIdVendor {
    pub fn new() -> UniqueIdVendor {
        UniqueIdVendor {
            // map: IndexMap::new(),
            avoid: HashSet::new(),
        }
    }

    // If desired_name is taken will add _2, _3, etc.
    pub fn get_uniq_id(&mut self, desired_name: &str) -> String {
        let mut name = desired_name.to_string();
        let mut i = 2;
        while self.avoid.contains(&name) {
            name = format!("{}_{}", desired_name, i);
            i += 1;
        }
        self.avoid(&name).unwrap();

        name
    }

    // Shortens name to a single letter, or more letters if necessary
    pub fn get_short_uniq_id(&mut self, desired_name: &str) -> String {
        let mut variants = UniqueIdVendor::all_prefixes(desired_name);
        variants.push(desired_name);

        self.get_one_of_uniq_id(variants)
    }

    pub fn avoid(&mut self, name: &str) -> Result<()> {
        if self.avoid.contains(name) {
            return Err(anyhow!(
                "avoid: {} is already reserved by someone else",
                name
            ));
        }
        self.avoid.insert(name.to_string());
        Ok(())
    }

    pub fn dont_avoid(&mut self, name: &str) -> Result<()> {
        if !self.avoid.contains(name) {
            return Err(anyhow!(
                "Unable to remove {} from avoid list - it's not there",
                name
            ));
        }
        self.avoid.remove(name);
        Ok(())
    }

    // Provided desired names ("n", "na", "nam") find available one
    // If none are available, will add _2, _3 to last option.
    fn get_one_of_uniq_id(&mut self, desired_names: Vec<&str>) -> String {
        for name in &desired_names {
            if self.avoid.contains(&name.to_string()) {
                continue;
            }
            if !self.avoid.contains(*name) {
                self.avoid.insert(name.to_string());
                return name.to_string();
            }
        }

        let last_option = desired_names.last().unwrap();
        self.get_uniq_id(last_option)
    }

    fn all_prefixes(name: &str) -> Vec<&str> {
        (1..name.len()).into_iter().map(|i| &name[..i]).collect()
    }

    // Check for identical keys in either the avoid set or map between two vendors
    pub fn has_conflict(&self, other: &UniqueIdVendor) -> bool {
        // Check if any key in self.avoid is in other.avoid or other.map
        for key in &self.avoid {
            if other.avoid.contains(key) {
                return true;
            }
        }

        false
    }

    pub fn merge(&mut self, other: &mut UniqueIdVendor) {
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
        let mut vendor3 = UniqueIdVendor::new();

        vendor1.avoid("conflict").unwrap();
        vendor2.avoid("conflict").unwrap();
        vendor3.get_uniq_id("conflict");

        assert!(vendor1.has_conflict(&vendor2));
        assert!(vendor1.has_conflict(&vendor3));
    }

    #[test]
    fn test_no_conflict() {
        let mut vendor1 = UniqueIdVendor::new();
        let mut vendor2 = UniqueIdVendor::new();
        let mut vendor3 = UniqueIdVendor::new();

        vendor1.avoid("unique1").unwrap();
        vendor2.avoid("unique2").unwrap();
        vendor3.get_uniq_id("unique3");

        assert!(!vendor1.has_conflict(&vendor2));
        assert!(!vendor1.has_conflict(&vendor3));
    }

    #[test]
    fn test_double_avoid() {
        let mut vendor = UniqueIdVendor::new();
        vendor.avoid("name").unwrap();
        assert!(vendor.avoid("name").is_err());
    }

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
        vendor.avoid("name").unwrap();

        assert_eq!(vendor.get_uniq_id("name"), "name_2");
    }

    #[test]
    fn test_one_of_uniq_id() {
        let mut vendor = UniqueIdVendor::new();
        vendor.avoid("nam").unwrap();

        assert_eq!(
            vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
            "n"
        );
        assert_eq!(
            vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
            "na"
        );
        // avoided!
        // assert_eq!(
        //     vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
        //     "nam"
        // );
        assert_eq!(
            vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
            "nam_2"
        );
        assert_eq!(
            vendor.get_one_of_uniq_id(UniqueIdVendor::all_prefixes("name")),
            "nam_3"
        );
    }

    #[test]
    fn test_short_uniq_id() {
        let mut vendor = UniqueIdVendor::new();

        assert_eq!(vendor.get_short_uniq_id("name"), "n");
        assert_eq!(vendor.get_short_uniq_id("name"), "na");
        assert_eq!(vendor.get_short_uniq_id("name"), "nam");
        assert_eq!(vendor.get_short_uniq_id("name"), "name");
        assert_eq!(vendor.get_short_uniq_id("name"), "name_2");
    }
}
