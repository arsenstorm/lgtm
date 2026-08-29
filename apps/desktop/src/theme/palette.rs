//! The two palettes.

use gpui::{rgb, rgba, Hsla};

use super::{Composer, Tokens};

/// `amount` of `base` mixed over `bg`, both packed `0xRRGGBB`. This is how
/// `@pierre/diffs` builds its row tints and gutter fills.
fn mix(bg: u32, base: u32, amount: f32) -> Hsla {
    let channel = |shift: u32| {
        let (b, f) = ((bg >> shift) & 0xff, (base >> shift) & 0xff);
        (b as f32 + (f as f32 - b as f32) * amount).round() as u32
    };
    rgb((channel(16) << 16) | (channel(8) << 8) | channel(0)).into()
}

fn dark_composer() -> Composer {
    Composer {
        rear: rgb(0x1f1f1f).into(),
        card: rgb(0x2a2a2a).into(),
        edge: rgb(0x2f2f2f).into(),
        placeholder: rgb(0x5f5f5f).into(),
        secondary: rgb(0x959595).into(),
        primary: rgb(0xf4f4f4).into(),
        divider: rgb(0x363636).into(),
        send_bg: rgb(0xffffff).into(),
        send_fg: rgb(0x000000).into(),
        send_disabled_bg: rgb(0x3a3a3a).into(),
        send_disabled_fg: rgb(0x8f8f8f).into(),
    }
}

pub fn dark() -> Tokens {
    const BG: u32 = 0x0a_0a_0a;
    const FG: u32 = 0xfa_fa_fa;
    const ADD: u32 = 0x5e_cc_71;
    const DEL: u32 = 0xff_67_62;
    Tokens {
        bg: rgb(BG).into(),
        fg: rgb(FG).into(),
        card: rgb(0x1b1b1b).into(),
        popover: rgb(0x1b1b1b).into(),
        primary: rgb(0xebebeb).into(),
        primary_fg: rgb(0x1b1b1b).into(),
        muted: rgb(0x2b2b2b).into(),
        muted_fg: rgb(0xa3a3a3).into(),
        border: rgba(0xffffff1a).into(),
        input: rgba(0xffffff26).into(),
        input_fill: rgba(0xffffff13).into(),
        ring: rgb(0x8a8a8a).into(),
        sidebar: rgb(0x1b1b1b).into(),
        sidebar_border: rgba(0xffffff1a).into(),
        success: rgb(0x34d399).into(),
        warning: rgb(0xfbbf24).into(),
        info: rgb(0xa78bfa).into(),
        danger: rgb(0xff5c5c).into(),
        diff_add: rgb(ADD).into(),
        diff_del: rgb(DEL).into(),
        diff_add_bg: mix(BG, ADD, 0.20),
        diff_add_emph: rgba(0x5ecc7133).into(),
        diff_del_bg: mix(BG, DEL, 0.20),
        diff_del_emph: rgba(0xff676233).into(),
        gutter: mix(BG, FG, 0.075),
        hunk_bg: mix(BG, FG, 0.075),
        selection: rgba(0xebebeb40).into(),
        overlay: rgba(0x00000099).into(),
        composer: dark_composer(),
    }
}

fn light_composer() -> Composer {
    Composer {
        rear: rgb(0xf3f3f3).into(),
        card: rgb(0xffffff).into(),
        edge: rgb(0xe6e6e6).into(),
        placeholder: rgb(0x9a9a9a).into(),
        secondary: rgb(0x6e6e6e).into(),
        primary: rgb(0x111111).into(),
        divider: rgb(0xdcdcdc).into(),
        send_bg: rgb(0x111111).into(),
        send_fg: rgb(0xffffff).into(),
        send_disabled_bg: rgb(0xe6e6e6).into(),
        send_disabled_fg: rgb(0x9a9a9a).into(),
    }
}

pub fn light() -> Tokens {
    const BG: u32 = 0xff_ff_ff;
    const FG: u32 = 0x0b_0b_0b;
    const ADD: u32 = 0x0d_be_4e;
    const DEL: u32 = 0xff_2e_3f;
    Tokens {
        bg: rgb(BG).into(),
        fg: rgb(FG).into(),
        card: rgb(0xffffff).into(),
        popover: rgb(0xffffff).into(),
        primary: rgb(0x1b1b1b).into(),
        primary_fg: rgb(0xfafafa).into(),
        muted: rgb(0xf5f5f5).into(),
        muted_fg: rgb(0x737373).into(),
        border: rgb(0xebebeb).into(),
        input: rgb(0xebebeb).into(),
        input_fill: rgb(0xf5f5f5).into(),
        ring: rgb(0xb4b4b4).into(),
        sidebar: rgb(0xfafafa).into(),
        sidebar_border: rgb(0xebebeb).into(),
        success: rgb(0x059669).into(),
        warning: rgb(0xd97706).into(),
        info: rgb(0x7c3aed).into(),
        danger: rgb(0xdc2626).into(),
        diff_add: rgb(ADD).into(),
        diff_del: rgb(DEL).into(),
        diff_add_bg: mix(BG, ADD, 0.12),
        diff_add_emph: rgba(0x0dbe4e26).into(),
        diff_del_bg: mix(BG, DEL, 0.12),
        diff_del_emph: rgba(0xff2e3f26).into(),
        gutter: mix(BG, FG, 0.015),
        hunk_bg: mix(BG, FG, 0.015),
        selection: rgba(0x1b1b1b26).into(),
        overlay: rgba(0x0b0b0b4d).into(),
        // The dark levels inverted around the page: the rear panel sits below
        // the page, the card on it.
        composer: light_composer(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(t: &Tokens) -> [Hsla; 38] {
        [
            t.bg,
            t.fg,
            t.card,
            t.popover,
            t.primary,
            t.primary_fg,
            t.muted,
            t.muted_fg,
            t.border,
            t.input,
            t.input_fill,
            t.ring,
            t.sidebar,
            t.sidebar_border,
            t.success,
            t.warning,
            t.info,
            t.danger,
            t.diff_add,
            t.diff_del,
            t.diff_add_bg,
            t.diff_add_emph,
            t.diff_del_bg,
            t.diff_del_emph,
            t.gutter,
            t.selection,
            t.overlay,
            t.composer.rear,
            t.composer.card,
            t.composer.edge,
            t.composer.placeholder,
            t.composer.secondary,
            t.composer.primary,
            t.composer.divider,
            t.composer.send_bg,
            t.composer.send_fg,
            t.composer.send_disabled_bg,
            t.composer.send_disabled_fg,
        ]
    }

    #[test]
    fn every_dark_token_is_set() {
        for color in all(&dark()) {
            assert_ne!(color, Hsla::default());
        }
    }

    #[test]
    fn every_light_token_is_set() {
        for color in all(&light()) {
            assert_ne!(color, Hsla::default());
        }
    }

    #[test]
    fn dark_and_light_differ() {
        assert_ne!(dark(), light());
    }

    #[test]
    fn mixing_stays_between_the_two_ends() {
        assert_eq!(mix(0x000000, 0xffffff, 0.0), rgb(0x000000).into());
        assert_eq!(mix(0x000000, 0xffffff, 1.0), rgb(0xffffff).into());
        assert_eq!(mix(0x000000, 0xffffff, 0.5), rgb(0x808080).into());
    }

    /// The row tint has to stay a tint: closer to the page than to the sign
    /// colour, or the diff turns into a wall of green and red.
    #[test]
    fn diff_row_tint_is_closer_to_the_background() {
        let t = dark();
        assert_eq!(t.diff_add_bg, mix(0x0a0a0a, 0x5ecc71, 0.2));
        assert_ne!(t.diff_add_bg, t.diff_add);
    }
}
