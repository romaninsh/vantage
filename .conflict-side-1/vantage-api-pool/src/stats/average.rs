use rust_decimal::prelude::*;
use std::time::Duration;

#[derive(Debug, Clone, Default, Copy)]
pub struct Average {
    total: Decimal,
    count: Decimal,
}

impl Average {
    pub fn new() -> Self {
        Self {
            total: Decimal::ZERO,
            count: Decimal::ZERO,
        }
    }

    pub fn add_sample(&mut self, value: Decimal) {
        self.total += value;
        self.count += Decimal::ONE;
    }

    pub fn get_value(&self) -> Decimal {
        if self.count > Decimal::ZERO {
            self.total / self.count
        } else {
            Decimal::ZERO
        }
    }

    pub fn reset(&mut self) {
        self.total = Decimal::ZERO;
        self.count = Decimal::ZERO;
    }

    pub fn from_duration(duration: Duration) -> Self {
        let mut avg = Self::new();
        avg.add_sample(Decimal::from(duration.as_millis() as u64));
        avg
    }
}

impl std::ops::Add for Average {
    type Output = Average;

    fn add(self, other: Average) -> Average {
        Average {
            total: self.total + other.total,
            count: self.count + other.count,
        }
    }
}

impl std::ops::Sub for Average {
    type Output = Average;

    fn sub(self, other: Average) -> Average {
        Average {
            total: self.total.saturating_sub(other.total),
            count: self.count.saturating_sub(other.count),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_average_operations() {
        // First second: 10 requests
        let mut first_second = Average::new();
        first_second.add_sample(dec!(10));
        assert_eq!(first_second.get_value(), dec!(10));

        // Second second: 9 requests
        let mut second_second = Average::new();
        second_second.add_sample(dec!(9));
        assert_eq!(second_second.get_value(), dec!(9));

        // Average RPS after 2 seconds: (10+9)/2 = 9.5
        let two_seconds = first_second.clone() + second_second.clone();
        assert_eq!(two_seconds.get_value(), dec!(9.5));

        // Third second: 8 requests
        let mut third_second = Average::new();
        third_second.add_sample(dec!(8));
        assert_eq!(third_second.get_value(), dec!(8));

        // Average RPS after 3 seconds: (10+9+8)/3 = 9
        let three_seconds = two_seconds + third_second;
        assert_eq!(three_seconds.get_value(), dec!(9));

        // Subtract first second: (9+8)/2 = 8.5
        let without_first = three_seconds - first_second;
        assert_eq!(without_first.get_value(), dec!(8.5));
    }
}
