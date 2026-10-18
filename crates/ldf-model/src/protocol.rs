//! LIN 프로토콜 계산: 보호된 ID(PID), 체크섬, 프레임 시간.

/// 6비트 프레임 ID에 패리티 2비트를 붙인 보호된 ID(PID).
///
/// `P0 = ID0 ^ ID1 ^ ID2 ^ ID4`, `P1 = !(ID1 ^ ID3 ^ ID4 ^ ID5)`.
pub fn protected_id(id: u8) -> u8 {
    let id = id & 0x3f;
    let bit = |n: u8| (id >> n) & 1;
    let p0 = bit(0) ^ bit(1) ^ bit(2) ^ bit(4);
    let p1 = (bit(1) ^ bit(3) ^ bit(4) ^ bit(5)) ^ 1;
    id | (p0 << 6) | (p1 << 7)
}

/// PID의 패리티가 맞는지.
pub fn pid_is_valid(pid: u8) -> bool {
    protected_id(pid & 0x3f) == pid
}

fn inverted_carry_sum(init: u16, data: &[u8]) -> u8 {
    let mut sum = init;
    for &b in data {
        sum += u16::from(b);
        if sum >= 256 {
            sum -= 255;
        }
    }
    // sum은 항상 0..=255
    !(sum as u8)
}

/// 클래식 체크섬(LIN 1.x): 데이터 바이트만 더한다.
pub fn classic_checksum(data: &[u8]) -> u8 {
    inverted_carry_sum(0, data)
}

/// 향상된 체크섬(LIN 2.x): PID를 포함해 더한다.
pub fn enhanced_checksum(pid: u8, data: &[u8]) -> u8 {
    inverted_carry_sum(u16::from(pid), data)
}

/// 프레임의 공칭 전송 비트 수: 헤더 34 + (데이터 N + 체크섬 1) × 10.
pub fn frame_bits_nominal(size_bytes: u32) -> u32 {
    34 + 10 * (size_bytes + 1)
}

/// 공칭 프레임 시간(ms). 속도가 0이면 `None`.
pub fn frame_time_nominal_ms(size_bytes: u32, speed_bps: u32) -> Option<f64> {
    (speed_bps > 0)
        .then(|| f64::from(frame_bits_nominal(size_bytes)) * 1000.0 / f64::from(speed_bps))
}

/// 최대 프레임 시간(ms): 공칭의 1.4배.
pub fn frame_time_max_ms(size_bytes: u32, speed_bps: u32) -> Option<f64> {
    frame_time_nominal_ms(size_bytes, speed_bps).map(|t| t * 1.4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_known_values() {
        assert_eq!(protected_id(0x2c), 0xec);
        assert_eq!(protected_id(0x3c), 0x3c); // MasterReq
        assert_eq!(protected_id(0x3d), 0x7d); // SlaveResp
        assert_eq!(protected_id(0x00), 0x80);
        assert_eq!(protected_id(0x3e), 0xfe);
        assert_eq!(protected_id(0x3f), 0xbf);
    }

    #[test]
    fn pid_parity_check() {
        for id in 0..64u8 {
            assert!(pid_is_valid(protected_id(id)));
        }
        assert!(!pid_is_valid(0x3c ^ 0x40));
    }

    #[test]
    fn checksum_examples() {
        // 합이 255를 넘지 않는 단순한 경우: ~(1+2+3) = 0xF9
        assert_eq!(classic_checksum(&[1, 2, 3]), 0xf9);
        // 캐리: 0xff + 0x01 = 0x100 -> 0x01(캐리 더함) -> ~0x01 = 0xfe
        assert_eq!(classic_checksum(&[0xff, 0x01]), 0xfe);
        // 향상: PID 포함
        assert_eq!(enhanced_checksum(0xec, &[]), !0xecu8);
        assert_eq!(
            enhanced_checksum(0x10, &[1, 2, 3]),
            classic_checksum(&[0x10, 1, 2, 3])
        );
    }

    #[test]
    fn frame_times_match_vector_reference() {
        // 19.2 kbps, 8바이트: 공칭 6.46 ms, 최대 9.04 ms (PRD FR-UI-04)
        let nom = frame_time_nominal_ms(8, 19_200).unwrap_or(0.0);
        let max = frame_time_max_ms(8, 19_200).unwrap_or(0.0);
        assert!((nom - 6.4583).abs() < 1e-3, "{nom}");
        assert!((max - 9.0417).abs() < 1e-3, "{max}");
        assert_eq!(frame_bits_nominal(8), 124);
        assert_eq!(frame_time_nominal_ms(8, 0), None);
    }
}
