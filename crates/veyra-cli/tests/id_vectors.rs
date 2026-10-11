use std::process::Command;

use veyra_core::ids::{ObjectAddress, ObjectId, RegionKey, UniverseId};

#[test]
fn id_command_derives_every_v1_birth_address_from_the_committed_universe_vector() {
    let universe = UniverseId([7; 32]);
    let cases = [
        (
            "system_seed:-1,64,0,300",
            ObjectAddress::SystemSeed {
                region: RegionKey { level: 0, ix: -1, iy: 64, iz: 0 },
                slot: 300,
            },
        ),
        (
            "body_in_system:obj:11111111111111111111111111111111,2,128",
            ObjectAddress::BodyInSystem { system: ObjectId([0x11; 16]), role: 2, ordinal: 128 },
        ),
        (
            "free_object:-2,3,-4,1",
            ObjectAddress::FreeObject {
                region: RegionKey { level: 0, ix: -2, iy: 3, iz: -4 },
                slot: 1,
            },
        ),
        ("fixture:cb0-addressing", ObjectAddress::Fixture { name: "cb0-addressing".to_owned() }),
    ];
    let universe_text = universe.text();
    for (text, address) in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_veyra"))
            .args(["id", "object", "--universe", universe_text.as_str(), "--address", text])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            ObjectId::derive(universe, &address).unwrap().to_string()
        );
    }
}
