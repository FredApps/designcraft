// Diagnostic-only addition, not a production test replacement. Existing tests stay intact.
#[test]
fn diagnostic_fogra39_profile_and_intent_values() {
    for bpc in [false, true] {
        let c = Cms::new(&ColorSettings { bpc, ..Default::default() }).unwrap();
        for (cmyk, want) in FOGRA39_ANCHORS {
            eprintln!("DIAGNOSTIC bpc={bpc} cmyk={cmyk:?} rgb={:?} existing_reference={want:?}", shown(&c, cmyk));
        }
        let rgb = [0.8, 0.25, 0.25];
        let original = lab::srgb_to_lab(rgb);
        let p = c.srgb_to_cmyk(rgb, Intent::Perceptual);
        let r = c.srgb_to_cmyk(rgb, Intent::RelativeColorimetric);
        let p_lab = c.cmyk_to_lab(p);
        let r_lab = c.cmyk_to_lab(r);
        eprintln!("DIAGNOSTIC bpc={bpc} rgb={rgb:?} perceptual={p:?} relative={r:?} perceptual_lab={p_lab:?} relative_lab={r_lab:?} perceptual_delta_e={} relative_delta_e={}", delta_e2000(p_lab, original), delta_e2000(r_lab, original));
    }
}
