//! 타이핑 효과(부드러운 커서·글자 스며들기)의 시간·범위 계산: 화면과 무관한 순수 함수.
//! 커서 이동 보간은 egui 내장 `animate_value_with_time`, 곡선은 `emath::easing`을 쓴다.

use eframe::egui::emath::easing;

/// 글자가 스며드는 시간(초).
pub const REVEAL_SECS: f32 = 0.12;
/// 한 번에 효과를 줄 수 있는 최대 글자 수(붙여넣기처럼 많이 들어오면 건너뛴다).
pub const MAX_TYPED: usize = 20;
/// 입력 직후 깜박이지 않고 선명하게 유지하는 시간(초).
const HOLD_SECS: f32 = 0.5;
/// 깜박임 한 주기(초).
const BLINK_PERIOD: f32 = 1.2;

/// 마지막 입력 뒤 `since_input`초에서의 커서 선명도(0.0~1.0). 입력 직후 0.5초는 항상 1.0이고,
/// 그 뒤로는 코사인 곡선으로 서서히 흐려졌다 선명해진다. `blink`가 거짓이면 항상 1.0.
pub fn blink_alpha(since_input: f32, blink: bool) -> f32 {
    if !blink || since_input <= HOLD_SECS {
        return 1.0;
    }
    let t = (since_input - HOLD_SECS) / BLINK_PERIOD * std::f32::consts::TAU;
    ((t.cos() + 1.0) / 2.0).clamp(0.0, 1.0)
}

/// 글자를 친 뒤 `age`초에서의 덮개 투명도: 1.0(안 보임)에서 0.0(선명)으로.
pub fn reveal_cover(age: f32) -> f32 {
    let t = age / REVEAL_SECS;
    if t >= 1.0 {
        0.0
    } else if t <= 0.0 {
        1.0
    } else {
        (1.0 - easing::exponential_out(t)).clamp(0.0, 1.0)
    }
}

/// 이번 프레임에 방금 친 글자 범위(글자 위치). 본문이 1~`MAX_TYPED`자 늘고 커서가 그만큼 오른쪽으로 갔을 때만 Some.
/// 삭제·붙여넣기(많은 글자)·커서만 움직인 경우·한글 조합 중 같은 음절을 바꿔 쓴 경우(길이 그대로)는 None.
pub fn typed_range(prev_len: usize, new_len: usize, prev_cursor: usize, new_cursor: usize) -> Option<(usize, usize)> {
    if new_len <= prev_len {
        return None;
    }
    let n = new_len - prev_len;
    if n > MAX_TYPED || new_cursor != prev_cursor + n {
        return None;
    }
    Some((prev_cursor, new_cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blink_is_solid_right_after_input_and_in_range() {
        assert_eq!(blink_alpha(0.0, true), 1.0);
        assert_eq!(blink_alpha(0.5, true), 1.0);
        for i in 0..400 {
            let a = blink_alpha(i as f32 * 0.01, true);
            assert!((0.0..=1.0).contains(&a), "{i}: {a}");
        }
        assert!(blink_alpha(0.5 + BLINK_PERIOD / 2.0, true) < 0.01, "주기 절반에서 가장 흐림");
        assert_eq!(blink_alpha(3.3, false), 1.0);
    }

    #[test]
    fn reveal_goes_from_covered_to_clear() {
        assert_eq!(reveal_cover(0.0), 1.0);
        assert_eq!(reveal_cover(-1.0), 1.0);
        assert_eq!(reveal_cover(REVEAL_SECS), 0.0);
        assert_eq!(reveal_cover(5.0), 0.0);
        let mut prev = 1.0;
        for i in 1..=12 {
            let c = reveal_cover(i as f32 * 0.01);
            assert!(c <= prev + 1e-6, "단조 감소해야 함");
            prev = c;
        }
    }

    #[test]
    fn typed_range_only_for_small_typing() {
        assert_eq!(typed_range(10, 11, 5, 6), Some((5, 6)));
        assert_eq!(typed_range(10, 12, 5, 7), Some((5, 7)));
        assert_eq!(typed_range(10, 10, 5, 5), None); // 한글 조합 중 음절 바꿔 쓰기
        assert_eq!(typed_range(10, 9, 5, 4), None); // 삭제
        assert_eq!(typed_range(10, 11, 5, 5), None); // 길이만 늘고 커서는 그대로(다른 곳에서 바뀜)
        assert_eq!(typed_range(0, 21, 0, 21), None); // 붙여넣기
        assert_eq!(typed_range(0, 20, 0, 20), Some((0, 20)));
    }
}
