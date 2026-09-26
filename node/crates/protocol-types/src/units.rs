//! Exact decimal conversion and the approved six-decimal native token units.

pub const DGT_DECIMALS: u8 = 6;
pub const DRT_DECIMALS: u8 = 6;
pub const DGT_BASE_DENOM: &str = "udgt";
pub const DRT_BASE_DENOM: &str = "udrt";
pub const DGT_BASE_UNITS_PER_TOKEN: u128 = 1_000_000;
pub const DRT_BASE_UNITS_PER_TOKEN: u128 = 1_000_000;
/// Approved fixed total in whole DGT. This constant does not authorize minting.
pub const DGT_TOTAL_TOKENS: u128 = 1_000_000_000;
pub const DGT_TOTAL_BASE_UNITS: u128 = DGT_TOTAL_TOKENS * DGT_BASE_UNITS_PER_TOKEN;
pub const DGT_SCALE: DecimalScale = DecimalScale {
    decimals: DGT_DECIMALS,
    multiplier: DGT_BASE_UNITS_PER_TOKEN,
};
pub const DRT_SCALE: DecimalScale = DecimalScale {
    decimals: DRT_DECIMALS,
    multiplier: DRT_BASE_UNITS_PER_TOKEN,
};

/// A decimal scale whose multiplier fits in the u128 amount representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecimalScale {
    decimals: u8,
    multiplier: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitError {
    UnsupportedScale,
    InvalidDecimal,
    ExcessPrecision,
    Overflow,
}

impl DecimalScale {
    /// Require the caller to supply the scale. No network policy is inferred.
    pub fn new(decimals: u8) -> Result<Self, UnitError> {
        let multiplier = 10u128
            .checked_pow(u32::from(decimals))
            .ok_or(UnitError::UnsupportedScale)?;
        Ok(Self {
            decimals,
            multiplier,
        })
    }

    /// Convert whole tokens to base units, rejecting overflow without rounding.
    pub fn whole_tokens_to_base_units(self, amount: u128) -> Result<u128, UnitError> {
        amount
            .checked_mul(self.multiplier)
            .ok_or(UnitError::Overflow)
    }

    /// Parse unsigned decimal text without rounding or floating-point arithmetic.
    /// Reject signs, whitespace, exponents, leading zeros and excess precision.
    pub fn to_base_units(self, value: &str) -> Result<u128, UnitError> {
        let (whole, fraction) = match value.split_once('.') {
            Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
            Some(_) => return Err(UnitError::InvalidDecimal),
            None => (value, ""),
        };
        if whole.is_empty()
            || (whole.len() > 1 && whole.starts_with('0'))
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(UnitError::InvalidDecimal);
        }
        if fraction.len() > usize::from(self.decimals) {
            return Err(UnitError::ExcessPrecision);
        }
        let whole = whole.parse::<u128>().map_err(|_| UnitError::Overflow)?;
        let fraction_value = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u128>().map_err(|_| UnitError::Overflow)?
        };
        // Both factors are bounded by the validated decimal scale.
        let padding = u32::from(self.decimals) - fraction.len() as u32;
        let fraction_units = fraction_value
            .checked_mul(10u128.pow(padding))
            .ok_or(UnitError::Overflow)?;
        whole
            .checked_mul(self.multiplier)
            .and_then(|v| v.checked_add(fraction_units))
            .ok_or(UnitError::Overflow)
    }

    /// Format base units as exact decimal text, omitting trailing fractional zeros.
    pub fn format_base_units(self, amount: u128) -> String {
        let whole = amount / self.multiplier;
        let remainder = amount % self.multiplier;
        if remainder == 0 {
            whole.to_string()
        } else {
            let fraction = format!("{:0width$}", remainder, width = usize::from(self.decimals));
            format!("{whole}.{}", fraction.trim_end_matches('0'))
        }
    }
}
