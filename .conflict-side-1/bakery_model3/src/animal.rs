use vantage_csv::{CsvType, type_system::CsvTypeAnimalMarker};
use vantage_types::TerminalRender;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Animal {
    #[default]
    Cat,
    Dog,
    Pig,
    Cow,
    Chicken,
}

impl Animal {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cat" => Some(Animal::Cat),
            "dog" => Some(Animal::Dog),
            "pig" => Some(Animal::Pig),
            "cow" => Some(Animal::Cow),
            "chicken" => Some(Animal::Chicken),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Animal::Cat => "cat",
            Animal::Dog => "dog",
            Animal::Pig => "pig",
            Animal::Cow => "cow",
            Animal::Chicken => "chicken",
        }
    }
}

impl TerminalRender for Animal {
    fn render(&self) -> String {
        match self {
            Animal::Cat => "🐱".to_string(),
            Animal::Dog => "🐶".to_string(),
            Animal::Pig => "🐷".to_string(),
            Animal::Cow => "🐮".to_string(),
            Animal::Chicken => "🐔".to_string(),
        }
    }
}

impl CsvType for Animal {
    type Target = CsvTypeAnimalMarker;

    fn to_csv_string(&self) -> String {
        self.as_str().to_string()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        Animal::from_str(&value)
    }
}
