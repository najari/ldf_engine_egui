use super::*;
use ldf_model::EncodingEntry;
use ldf_syntax::Severity;

/// LIN 2.0 형식: `message_ID`로 프레임을 가리키고 이벤트 트리거는 메시지 ID로 잇는다(직접 쓴 시료).
const NCF_2_0: &str = r#"
/* Node Capability file */
node_capability_file;

LIN_language_version = "2.0";

node Motor2_0 {
 general {
  LIN_protocol_version = "2.0";
  supplier = 0x1e;
  function = 0x1;
  variant = 0;
  bitrate = 19200;
 }
 diagnostic {
  NAD = 0x1;
  P2_min = 0 ms;
 }
 frames {
  subscribe Control {
   message_ID = 0x1234;
   length = 8;
   signals {
    Speed {init_value = 16; size = 16; offset = 16;
    }
   }
  }
  publish State {
   message_ID = 0x2222;
   length = 8;
   event_triggered_message_ID = 0x4567;
   signals {
    Temp {init_value = 0; size = 8; offset = 8;
    }
    LinError {init_value = 0; size = 1; offset = 40;
    }
   }
  }
  publish StateEvent {
   message_ID = 0x4567;
   length = 8;
  }
 }
 status_management {
  error_response = LinError;
 }
}
"#;

/// ISO 17987 형식: 인코딩 섹션, 시그널 안의 인코딩 참조, 바이트 배열 초기값, 프레임 이름으로 잇는 이벤트 트리거.
const NCF_ISO: &str = r#"
node_capability_file;
LIN_language_version = "ISO17987:2015";
node MotorIso {
 general {
  LIN_protocol_version = "ISO17987:2015";
  supplier = 0x1e;
  function = 0x2;
  variant = 1;
  bitrate = 19.2 kbps;
  sends_wake_up_signal = "yes";
 }
 diagnostic {
  NAD = 0x5;
  diagnostic_class = 1;
 }
 frames {
  subscribe Query {
   length = 5;
   signals {
    Block {init_value = {5, 4, 3, 2, 1}; size = 40; offset = 0;
    }
   }
  }
  publish Cycl {
   length = 8;
   event_triggered_frame = CyclEvent;
   min_period = 10 ms;
   max_period = 0.1 s;
   signals {
    Temp {init_value = 0; size = 8; offset = 8;
          encTemperature;
    }
    LinError {init_value = 0; size = 1; offset = 40;
              encoding encError {logical_value, 0, "ok"; logical_value, 1, "error";};
    }
   }
  }
  publish CyclEvent {
   length = 8;
  }
 }
 encoding {
  encTemperature {
   physical_value, 0, 200, 0.500, -20.000, "Degree";
   logical_value, 255, "invalid";
  }
 }
 status_management {
  error_response = LinError;
 }
 free_text {
  "second line"
 }
}
"#;

fn read(src: &str) -> Parsed {
    parse(src)
}

fn messages(p: &Parsed) -> Vec<&str> {
    p.diagnostics.iter().map(|d| d.message.as_str()).collect()
}

#[test]
fn reads_a_lin_2_0_node_capability_file() {
    let p = read(NCF_2_0);
    assert!(p.diagnostics.is_empty(), "{:?}", messages(&p));
    assert_eq!(p.file.language_version.as_deref(), Some("2.0"));
    let n = &p.file.nodes[0];
    assert_eq!(n.name, "Motor2_0");
    assert_eq!(n.general.protocol_version.as_deref(), Some("2.0"));
    assert_eq!(
        (n.general.supplier, n.general.function, n.general.variant),
        (Some(0x1e), Some(1), Some(0))
    );
    // 단위 없는 19200은 bit/s다
    assert_eq!(n.general.bitrate, Some(Bitrate::Fixed { bps: 19200.0 }));
    assert_eq!(n.diagnostic.nad, Some(Nad::List(vec![1])));
    assert_eq!(n.diagnostic.p2_min_ms, Some(0.0));

    assert_eq!(n.frames.len(), 3);
    let control = &n.frames[0];
    assert_eq!(
        (control.direction, control.name.as_str()),
        (Direction::Subscribe, "Control")
    );
    assert_eq!(
        (control.message_id, control.length),
        (Some(0x1234), Some(8))
    );
    let speed = &control.signals[0];
    assert_eq!(
        (speed.init.clone(), speed.size, speed.offset),
        (Some(InitValue::Scalar(16)), Some(16), Some(16))
    );
    let state = &n.frames[1];
    assert_eq!(state.direction, Direction::Publish);
    assert_eq!(
        state.event_triggered,
        Some(EventTriggered::MessageId(0x4567))
    );
    assert_eq!(state.signals.len(), 2);
    assert_eq!(
        n.status_management,
        vec![Item {
            key: "error_response".into(),
            value: "LinError".into(),
            span: n.status_management[0].span
        }]
    );
}

#[test]
fn reads_an_iso_file_with_encodings_and_array_initial_values() {
    let p = read(NCF_ISO);
    assert!(p.diagnostics.is_empty(), "{:?}", messages(&p));
    let n = &p.file.nodes[0];
    assert_eq!(n.general.bitrate, Some(Bitrate::Fixed { bps: 19200.0 }));
    assert_eq!(n.general.sends_wake_up_signal.as_deref(), Some("yes"));
    assert_eq!(n.diagnostic.diagnostic_class, Some(1));

    let query = &n.frames[0].signals[0];
    assert_eq!(query.init, Some(InitValue::Array(vec![5, 4, 3, 2, 1])));
    let cycl = &n.frames[1];
    assert_eq!(
        cycl.event_triggered,
        Some(EventTriggered::Frame("CyclEvent".into()))
    );
    assert_eq!(
        (cycl.min_period_ms, cycl.max_period_ms),
        (Some(10.0), Some(100.0))
    );
    // 이름으로 가리키는 인코딩과 그 자리에서 정의한 인코딩
    assert_eq!(
        cycl.signals[0].encoding,
        Some(EncodingRef::Named("encTemperature".into()))
    );
    let Some(EncodingRef::Inline(inline)) = &cycl.signals[1].encoding else {
        panic!("{:?}", cycl.signals[1].encoding);
    };
    assert_eq!(inline.name, "encError");
    assert_eq!(inline.entries.len(), 2);
    assert!(matches!(
        &inline.entries[1],
        EncodingEntry::Logical { value: 1, text } if text == "error"
    ));
    // 인코딩 섹션과 이름으로 찾기(시그널 안의 것도 찾는다)
    let temperature = n.find_encoding("encTemperature").expect("encTemperature");
    assert_eq!(temperature.entries.len(), 2);
    assert!(n.find_encoding("encError").is_some());
    assert!(n.find_encoding("nope").is_none());
    assert_eq!(n.free_text.as_deref(), Some("second line"));
}

#[test]
fn bitrate_forms() {
    let one = |value: &str| {
        let src = format!("node_capability_file; node N {{ general {{ bitrate = {value}; }} }}");
        let p = read(&src);
        (p.file.nodes[0].general.bitrate.clone(), p.diagnostics.len())
    };
    assert_eq!(one("19.2 kbps"), (Some(Bitrate::Fixed { bps: 19200.0 }), 0));
    assert_eq!(one("10417"), (Some(Bitrate::Fixed { bps: 10417.0 }), 0));
    assert_eq!(one("9600 bps"), (Some(Bitrate::Fixed { bps: 9600.0 }), 0));
    // 단위가 없는 1000 미만은 kbps로 본다
    assert_eq!(one("19.2"), (Some(Bitrate::Fixed { bps: 19200.0 }), 0));
    assert_eq!(
        one("automatic"),
        (
            Some(Bitrate::Automatic {
                min_bps: None,
                max_bps: None
            }),
            0
        )
    );
    assert_eq!(
        one("automatic min 10 kbps max 20 kbps"),
        (
            Some(Bitrate::Automatic {
                min_bps: Some(10_000.0),
                max_bps: Some(20_000.0)
            }),
            0
        )
    );
    assert_eq!(
        one("select { 2.4 kbps, 9.6 kbps }"),
        (
            Some(Bitrate::Select {
                bps: vec![2400.0, 9600.0]
            }),
            0
        )
    );
    assert_eq!(
        one("select { 2.4, 9600 }"),
        (
            Some(Bitrate::Select {
                bps: vec![2400.0, 9600.0]
            }),
            0
        )
    );
    // 읽을 수 없으면 진단하고 원문으로 남긴다
    let p = read("node_capability_file; node N { general { bitrate = fast; } }");
    let g = &p.file.nodes[0].general;
    assert_eq!(g.bitrate, None);
    assert_eq!((g.extra.len(), p.diagnostics.len()), (1, 1));
    assert_eq!(p.diagnostics[0].code, codes::BAD_VALUE);
    assert_eq!(p.diagnostics[0].severity, Severity::Warning);
}

#[test]
fn diagnostic_section_forms() {
    let p = read(
        "node_capability_file; node N { diagnostic {\n\
         NAD = 1 to 5; diagnostic_class = 2; P2_min = 50 ms; ST_min = 0.02 s;\n\
         N_As_timeout = 1000 ms; N_Cr_timeout = 1 s; max_message_length = 4095;\n\
         support_sid { 0xB0; 0xB2; // 설명\n }\n\
         } }",
    );
    assert!(p.diagnostics.is_empty(), "{:?}", messages(&p));
    let d = &p.file.nodes[0].diagnostic;
    assert_eq!(d.nad, Some(Nad::Range { from: 1, to: 5 }));
    assert_eq!(d.diagnostic_class, Some(2));
    assert_eq!((d.p2_min_ms, d.st_min_ms), (Some(50.0), Some(20.0)));
    assert_eq!(
        (d.n_as_timeout_ms, d.n_cr_timeout_ms),
        (Some(1000.0), Some(1000.0))
    );
    assert_eq!(d.max_message_length, Some(4095));
    assert_eq!(d.support_sid, vec![0xB0, 0xB2]);

    let list = read("node_capability_file; node N { diagnostic { NAD = 1, 2, 3; } }");
    assert_eq!(
        list.file.nodes[0].diagnostic.nad,
        Some(Nad::List(vec![1, 2, 3]))
    );
}

#[test]
fn unknown_items_are_kept_and_reported_as_information() {
    let p = read(
        "node_capability_file; node N {\n\
         general { response_tolerance = 40 %; supplier = 0x1e; }\n\
         frames { publish F { length = 2; frame_id = 0x10; } }\n\
         vendor { a = 1; }\n\
         }",
    );
    let n = &p.file.nodes[0];
    assert_eq!(n.general.supplier, Some(0x1e));
    assert_eq!(
        n.general
            .extra
            .iter()
            .map(|i| (&i.key[..], &i.value[..]))
            .collect::<Vec<_>>(),
        [("response_tolerance", "40 %")]
    );
    assert_eq!(n.frames[0].extra[0].key, "frame_id");
    assert_eq!(n.extra[0].key, "vendor");
    assert_eq!(n.extra[0].value, "{ a = 1; }");
    assert_eq!(p.diagnostics.len(), 3);
    assert!(
        p.diagnostics
            .iter()
            .all(|d| d.code == codes::UNKNOWN_ITEM && d.severity == Severity::Info)
    );
}

#[test]
fn malformed_input_is_diagnosed_and_never_panics() {
    // NCF가 아닌 LDF
    let p = read("LIN_description_file;\nNodes { Master: M, 1 ms, 0 ms; Slaves: A; }\n");
    assert!(p.diagnostics.iter().any(|d| d.code == codes::NOT_NCF));
    assert!(p.diagnostics.iter().any(|d| d.code == codes::NO_NODE));
    // 잘못된 문장 모양들
    let p = read(
        "node_capability_file; node N {\n\
         frames { publish { } listen F { } publish G { signals { 1 { } } } }\n\
         encoding { E { logical_value, x; bogus; physical_value, 1; } }\n\
         }",
    );
    assert!(p.diagnostics.iter().all(|d| d.severity != Severity::Error));
    assert!(
        p.diagnostics
            .iter()
            .filter(|d| d.code == codes::BAD_STRUCTURE)
            .count()
            >= 5,
        "{:?}",
        messages(&p)
    );
    for src in [
        "",
        "node_capability_file",
        "node N {",
        "}}} {{{ \"",
        "node_capability_file; node",
    ] {
        let _ = read(src);
    }
}

#[test]
fn is_ncf_looks_at_the_first_token_only() {
    assert!(is_ncf("/* c */ node_capability_file;"));
    assert!(is_ncf("// 주석\nnode_capability_file;\nnode N { }"));
    assert!(!is_ncf("LIN_description_file;"));
    assert!(!is_ncf(""));
    assert!(!is_ncf("Nodes { }\nnode_capability_file;"));
}

#[test]
fn signals_that_overflow_their_frame_are_listed() {
    let p = read(
        "node_capability_file; node N { frames { publish F { length = 2; signals {\
         A { size = 8; offset = 8; } B { size = 8; offset = 12; } } } } }",
    );
    assert_eq!(p.file.nodes[0].overflowing_signals(), vec![("F", "B")]);
    assert!(read(NCF_2_0).file.nodes[0].overflowing_signals().is_empty());
}

/// CANoe 샘플 구성의 NCF가 있으면(`LDF_CANOE_SAMPLES`) 모두 경고 없이 읽히는지 본다.
#[test]
fn canoe_sample_files_are_read_without_warnings() {
    let Some(dir) = std::env::var_os("LDF_CANOE_SAMPLES") else {
        return;
    };
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ncf")) {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(std::path::Path::new(&dir), &mut files);
    assert!(!files.is_empty(), "샘플 NCF를 찾지 못했다");
    for f in files {
        let bytes = std::fs::read(&f).expect("read");
        let text: String = bytes.iter().map(|&b| char::from(b)).collect();
        let p = parse(&text);
        let loud: Vec<_> = p
            .diagnostics
            .iter()
            .filter(|d| d.severity != Severity::Info)
            .collect();
        assert!(loud.is_empty(), "{}: {loud:?}", f.display());
        assert!(is_ncf(&text), "{}", f.display());
        assert!(!p.file.nodes.is_empty(), "{}", f.display());
        assert!(
            p.file.nodes[0].overflowing_signals().is_empty(),
            "{}",
            f.display()
        );
    }
}

#[test]
fn values_are_described_in_short_language_neutral_text() {
    assert_eq!(format_bps(19200.0), "19.2 kbps");
    assert_eq!(format_bps(10417.0), "10.417 kbps");
    assert_eq!(format_bps(600.0), "600 bps");
    assert_eq!(format_ms(0.5), "0.5 ms");
    assert_eq!(hex(0x1e, 4), "0x001E");
    assert_eq!(hex(0x12345, 2), "0x12345");
    assert_eq!(Bitrate::Fixed { bps: 19200.0 }.describe(), "19.2 kbps");
    assert_eq!(
        Bitrate::Automatic {
            min_bps: Some(10_000.0),
            max_bps: None
        }
        .describe(),
        "automatic 10 kbps – …"
    );
    assert_eq!(
        Bitrate::Select {
            bps: vec![2400.0, 9600.0]
        }
        .describe(),
        "select 2.4 kbps | 9.6 kbps"
    );
    assert_eq!(Nad::List(vec![1, 2]).describe(), "0x01, 0x02");
    assert_eq!(Nad::Range { from: 1, to: 5 }.describe(), "0x01 to 0x05");
    assert_eq!(EventTriggered::MessageId(0x4567).describe(), "0x4567");
    assert_eq!(describe_init(&InitValue::Array(vec![5, 4])), "{5, 4}");
    assert_eq!(describe_init(&InitValue::Scalar(16)), "16");
    assert_eq!(
        describe_entry(&EncodingEntry::Logical {
            value: 1,
            text: "on".into()
        }),
        "logical_value 1 \"on\""
    );
    assert_eq!(
        describe_entry(&EncodingEntry::Physical {
            min: 0,
            max: 200,
            scale: 0.5,
            offset: -20.0,
            unit: "Degree".into()
        }),
        "physical_value 0..200, × 0.5, + -20 [Degree]"
    );
}
