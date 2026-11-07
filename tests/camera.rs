use tacit::camera::Camera;
#[test]
fn cursor_anchored_zoom_keeps_world_point_fixed_at_limits() {
    let mut c = Camera {
        zoom: 0.8,
        pan: [125.0, -70.0],
    };
    let anchor = [370.0, 280.0];
    let world = [
        (anchor[0] - c.pan[0]) / c.zoom,
        (anchor[1] - c.pan[1]) / c.zoom,
    ];
    for factor in [1.2, 0.7, 100.0, 0.001] {
        c.zoom_at(factor, anchor);
        for i in 0..2 {
            assert!((world[i] * c.zoom + c.pan[i] - anchor[i]).abs() < 0.001);
        }
        assert!(c.valid());
    }
}
#[test]
fn fit_centers_content_with_margin_and_rejects_invalid_input() {
    let mut c = Camera::default();
    c.fit([1200.0, 800.0], [820.0, 500.0]);
    assert!((c.pan[1] - 24.0).abs() < 0.001);
    assert!(c.pan[0] >= 24.0);
    let before = c;
    c.zoom_at(f32::NAN, [0.0; 2]);
    assert_eq!(c, before);
    c.fit([0.0, 800.0], [820.0, 500.0]);
    assert_eq!(c, before);
}
#[test]
fn workspace_camera_survives_journal_reopen_and_legacy_default() {
    use tacit::{
        model::Session,
        presentation::{Scene, Workspace},
        timeline::Journal,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut j = Journal::open(&path, Session::default()).unwrap();
    let scene = Scene {
        workspace: Some(Workspace {
            camera: Camera {
                zoom: 0.6,
                pan: [-70.0, 90.0],
            },
            expanded: true,
            following: false,
            transcript_scroll: [0.0, 180.0],
        }),
        ..Scene::default()
    };
    j.present_scene(10, scene.clone()).unwrap();
    drop(j);
    let j = Journal::read(&path).unwrap();
    assert_eq!(j.replay_scene_at(10).unwrap().workspace, scene.workspace);
    let old = serde_json::to_value(Scene::default()).unwrap();
    assert!(old.get("workspace").is_none());
}
