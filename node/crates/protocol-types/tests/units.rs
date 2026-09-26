use dytallix_protocol_types::units::{
    DecimalScale, UnitError, DGT_BASE_DENOM, DGT_DECIMALS, DGT_SCALE, DGT_TOTAL_BASE_UNITS,
    DRT_BASE_DENOM, DRT_DECIMALS, DRT_SCALE,
};

#[test]
fn approved_native_units_have_exact_supply_and_precision() {
    assert_eq!((DGT_DECIMALS, DRT_DECIMALS), (6, 6));
    assert_eq!((DGT_BASE_DENOM, DRT_BASE_DENOM), ("udgt", "udrt"));
    assert_eq!(DGT_TOTAL_BASE_UNITS, 1_000_000_000_000_000);
    assert_eq!(
        DGT_SCALE.to_base_units("1000000000"),
        Ok(DGT_TOTAL_BASE_UNITS)
    );
    for scale in [DGT_SCALE, DRT_SCALE] {
        assert_eq!(scale.to_base_units("0.000001"), Ok(1));
        assert_eq!(
            scale.to_base_units("0.0000001"),
            Err(UnitError::ExcessPrecision)
        );
        assert_eq!(scale.whole_tokens_to_base_units(1), Ok(1_000_000));
        let largest_whole = u128::MAX / 1_000_000;
        assert_eq!(
            scale.whole_tokens_to_base_units(largest_whole),
            Ok(largest_whole * 1_000_000)
        );
        assert_eq!(
            scale.whole_tokens_to_base_units(largest_whole + 1),
            Err(UnitError::Overflow)
        );
    }
}

#[test]
fn explicit_scale_vectors() {
    for (decimals, text, expected) in [
        (0, "1000000000", 1_000_000_000),
        (6, "1000000000", 1_000_000_000_000_000),
        (18, "1000000000", 1_000_000_000_000_000_000_000_000_000),
        (6, "0.000001", 1),
        (6, "1.25", 1_250_000),
        (0, "0", 0),
    ] {
        assert_eq!(
            DecimalScale::new(decimals).unwrap().to_base_units(text),
            Ok(expected)
        );
    }
}

#[test]
fn reject_rounding_invalid_text_and_overflow() {
    let scale = DecimalScale::new(6).unwrap();
    for text in [
        "", "-1", "+1", " 1", "1 ", "1e6", "NaN", ".5", "1.", "01", "1.2.3", "１",
    ] {
        assert_eq!(
            scale.to_base_units(text),
            Err(UnitError::InvalidDecimal),
            "{text}"
        );
    }
    assert_eq!(
        scale.to_base_units("0.0000001"),
        Err(UnitError::ExcessPrecision)
    );
    assert_eq!(
        scale.to_base_units("1.0000000"),
        Err(UnitError::ExcessPrecision)
    );
    assert_eq!(
        scale.to_base_units(&u128::MAX.to_string()),
        Err(UnitError::Overflow)
    );
    assert_eq!(DecimalScale::new(39), Err(UnitError::UnsupportedScale));
    assert_eq!(DecimalScale::new(255), Err(UnitError::UnsupportedScale));
    assert_eq!(
        DecimalScale::new(0)
            .unwrap()
            .to_base_units("340282366920938463463374607431768211456"),
        Err(UnitError::Overflow)
    );
    assert_eq!(
        DecimalScale::new(38)
            .unwrap()
            .to_base_units("3.40282366920938463463374607431768211456"),
        Err(UnitError::Overflow)
    );
}

#[test]
fn all_supported_scales_round_trip_boundaries() {
    for decimals in 0..=38 {
        let scale = DecimalScale::new(decimals).unwrap();
        for amount in [0, 1, 9, 10, 999_999, 1_000_000, u128::MAX - 1, u128::MAX] {
            let text = scale.format_base_units(amount);
            assert_eq!(scale.to_base_units(&text), Ok(amount), "{decimals}: {text}");
        }
        let overflow = format!("{}0", scale.format_base_units(u128::MAX));
        assert!(scale.to_base_units(&overflow).is_err());
    }
}
