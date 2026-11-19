//! Smoke test: the mock core (ADR 0009) compiles, loads and runs. One `Core` per process (ADR 0008).

#[path = "../../../tests/support/mock_core.rs"]
mod mock_core;

#[cfg(unix)]
mod unix_tests {
    use super::mock_core::{self, Variant};
    use emu_libretro::{Core, CoreConfig, FrontendHandler, InputQuery, VideoFrame};

    #[derive(Default)]
    struct Probe {
        frames: u32,
        size: (u32, u32),
        samples: usize,
    }

    impl FrontendHandler for Probe {
        fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>) {
            if let Some(f) = frame {
                self.frames += 1;
                self.size = (f.width, f.height);
            }
        }
        fn audio_samples(&mut self, samples: &[i16]) {
            self.samples += samples.len();
        }
        fn input_poll(&mut self) {}
        fn input_state(&mut self, _query: InputQuery) -> i16 {
            0
        }
    }

    #[test]
    fn mock_core_loads_and_runs() {
        let lib = mock_core::build(Variant::Default);
        assert!(lib.is_file());

        let out = mock_core::output_dir().join("mock-smoke");
        std::fs::create_dir_all(&out).unwrap();
        let rom = out.join("game.mock");
        std::fs::write(&rom, b"mock content").unwrap();

        let config = CoreConfig {
            system_dir: out.join("system"),
            save_dir: out.join("saves"),
            assets_dir: out.join("assets"),
            variables: Vec::new(),
        };
        let mut core = Core::load(&lib, config).expect("load mock core");
        assert_eq!(core.system_info().library_name, "Mock Core");
        let av = core.load_game(&rom).expect("load game");
        assert_eq!((av.geometry.base_width, av.geometry.base_height), (64, 48));

        let mut probe = Probe::default();
        for _ in 0..3 {
            core.run(&mut probe).expect("run");
        }
        assert_eq!(probe.frames, 3);
        assert_eq!(probe.size, (64, 48));
        assert_eq!(probe.samples, 3 * 735 * 2);
    }
}
