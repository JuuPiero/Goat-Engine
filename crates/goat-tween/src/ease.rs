//! Easing functions (DOTween's / Robert Penner's set). `t` in 0..1 → eased 0..1 (Back / Elastic
//! overshoot).

use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ease {
    Linear,
    InSine,
    OutSine,
    InOutSine,
    InQuad,
    #[default]
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InOutCubic,
    InQuart,
    OutQuart,
    InOutQuart,
    InQuint,
    OutQuint,
    InOutQuint,
    InExpo,
    OutExpo,
    InOutExpo,
    InCirc,
    OutCirc,
    InOutCirc,
    InElastic,
    OutElastic,
    InOutElastic,
    InBack,
    OutBack,
    InOutBack,
    InBounce,
    OutBounce,
    InOutBounce,
}

pub const ALL: [Ease; 31] = [
    Ease::Linear,
    Ease::InSine,
    Ease::OutSine,
    Ease::InOutSine,
    Ease::InQuad,
    Ease::OutQuad,
    Ease::InOutQuad,
    Ease::InCubic,
    Ease::OutCubic,
    Ease::InOutCubic,
    Ease::InQuart,
    Ease::OutQuart,
    Ease::InOutQuart,
    Ease::InQuint,
    Ease::OutQuint,
    Ease::InOutQuint,
    Ease::InExpo,
    Ease::OutExpo,
    Ease::InOutExpo,
    Ease::InCirc,
    Ease::OutCirc,
    Ease::InOutCirc,
    Ease::InElastic,
    Ease::OutElastic,
    Ease::InOutElastic,
    Ease::InBack,
    Ease::OutBack,
    Ease::InOutBack,
    Ease::InBounce,
    Ease::OutBounce,
    Ease::InOutBounce,
];

impl Ease {
    /// By name (`"OutQuad"`, case-insensitive; `"InOutSine"`...).
    pub fn parse(name: &str) -> Option<Ease> {
        ALL.iter().copied().find(|e| format!("{e:?}").eq_ignore_ascii_case(name))
    }

    pub fn eval(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        const S: f32 = 1.70158;
        const S2: f32 = S * 1.525;
        match self {
            Ease::Linear => t,
            Ease::InSine => 1.0 - (t * PI / 2.0).cos(),
            Ease::OutSine => (t * PI / 2.0).sin(),
            Ease::InOutSine => -((PI * t).cos() - 1.0) / 2.0,
            Ease::InQuad => t * t,
            Ease::OutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Ease::InOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Ease::InCubic => t * t * t,
            Ease::OutCubic => 1.0 - (1.0 - t).powi(3),
            Ease::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Ease::InQuart => t.powi(4),
            Ease::OutQuart => 1.0 - (1.0 - t).powi(4),
            Ease::InOutQuart => {
                if t < 0.5 {
                    8.0 * t.powi(4)
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(4) / 2.0
                }
            }
            Ease::InQuint => t.powi(5),
            Ease::OutQuint => 1.0 - (1.0 - t).powi(5),
            Ease::InOutQuint => {
                if t < 0.5 {
                    16.0 * t.powi(5)
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(5) / 2.0
                }
            }
            Ease::InExpo => {
                if t == 0.0 {
                    0.0
                } else {
                    2f32.powf(10.0 * t - 10.0)
                }
            }
            Ease::OutExpo => {
                if t == 1.0 {
                    1.0
                } else {
                    1.0 - 2f32.powf(-10.0 * t)
                }
            }
            Ease::InOutExpo => {
                if t == 0.0 || t == 1.0 {
                    t
                } else if t < 0.5 {
                    2f32.powf(20.0 * t - 10.0) / 2.0
                } else {
                    (2.0 - 2f32.powf(-20.0 * t + 10.0)) / 2.0
                }
            }
            Ease::InCirc => 1.0 - (1.0 - t * t).max(0.0).sqrt(),
            Ease::OutCirc => (1.0 - (t - 1.0).powi(2)).max(0.0).sqrt(),
            Ease::InOutCirc => {
                if t < 0.5 {
                    (1.0 - (1.0 - (2.0 * t).powi(2)).max(0.0).sqrt()) / 2.0
                } else {
                    ((1.0 - (-2.0 * t + 2.0).powi(2)).max(0.0).sqrt() + 1.0) / 2.0
                }
            }
            Ease::InElastic => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    -(2f32.powf(10.0 * t - 10.0)) * ((t * 10.0 - 10.75) * (2.0 * PI / 3.0)).sin()
                }
            }
            Ease::OutElastic => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * (2.0 * PI / 3.0)).sin() + 1.0
                }
            }
            Ease::InOutElastic => {
                let c = 2.0 * PI / 4.5;
                if t == 0.0 || t == 1.0 {
                    t
                } else if t < 0.5 {
                    -(2f32.powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * c).sin()) / 2.0
                } else {
                    2f32.powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * c).sin() / 2.0 + 1.0
                }
            }
            Ease::InBack => (S + 1.0) * t * t * t - S * t * t,
            Ease::OutBack => 1.0 + (S + 1.0) * (t - 1.0).powi(3) + S * (t - 1.0).powi(2),
            Ease::InOutBack => {
                if t < 0.5 {
                    ((2.0 * t).powi(2) * ((S2 + 1.0) * 2.0 * t - S2)) / 2.0
                } else {
                    ((2.0 * t - 2.0).powi(2) * ((S2 + 1.0) * (t * 2.0 - 2.0) + S2) + 2.0) / 2.0
                }
            }
            Ease::InBounce => 1.0 - out_bounce(1.0 - t),
            Ease::OutBounce => out_bounce(t),
            Ease::InOutBounce => {
                if t < 0.5 {
                    (1.0 - out_bounce(1.0 - 2.0 * t)) / 2.0
                } else {
                    (1.0 + out_bounce(2.0 * t - 1.0)) / 2.0
                }
            }
        }
    }
}

fn out_bounce(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984375
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_ease_starts_at_0_and_ends_at_1() {
        for e in ALL {
            assert!(e.eval(0.0).abs() < 1e-4, "{e:?} {}", e.eval(0.0));
            assert!((e.eval(1.0) - 1.0).abs() < 1e-4, "{e:?} {}", e.eval(1.0));
            assert_eq!(Ease::parse(&format!("{e:?}").to_lowercase()), Some(e));
        }
        assert!(Ease::OutBack.eval(0.7) > 1.0);
        assert!(Ease::InQuad.eval(0.5) < 0.5 && Ease::OutQuad.eval(0.5) > 0.5);
    }
}
