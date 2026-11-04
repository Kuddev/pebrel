use super::*;

#[test]
fn configured_weight_preserves_families_styles_and_ligature_preferences() {
    let families = ["Normal", "Bold", "Italic", "Bold Italic"].map(str::to_owned);
    for (value, normal, bold) in [
        ("100", 100.0, 400.0),
        ("200", 200.0, 500.0),
        ("300", 300.0, 600.0),
        ("400", 400.0, 700.0),
        ("500", 500.0, 800.0),
        ("600", 600.0, 900.0),
        ("700", 700.0, 900.0),
        ("800", 800.0, 900.0),
        ("900", 900.0, 900.0),
    ] {
        for ligatures in [false, true] {
            let weight = TerminalFontWeight::from_settings(value).unwrap();
            let fonts = terminal_fonts(&families, weight, ligatures);
            for (index, font) in fonts.iter().enumerate() {
                assert_eq!(font.family.as_ref(), families[index]);
                assert_eq!(font.weight, FontWeight(if index % 2 == 0 { normal } else { bold }));
                assert_eq!(
                    font.style,
                    if index < 2 { FontStyle::Normal } else { FontStyle::Italic }
                );
                for feature in ["calt", "liga", "clig"] {
                    assert!(
                        font.features
                            .tag_value_list()
                            .contains(&(feature.to_owned(), u32::from(ligatures),))
                    );
                }
                assert!(font.features.tag_value_list().contains(&("kern".to_owned(), 0)));
            }
        }
    }
}
