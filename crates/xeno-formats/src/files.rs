//! Which original files make up a game installation.

/// Files the game needs, as named in a stock install (lowercase).
///
/// This is every file the installer copies except its own (`*.bat` and the
/// `xenobot.1` / `xenobot.2` disk markers), the placeholder `orders.txt`, and three
/// files the game never needs (T-024): `default.map` and `backup.map`, which only the
/// map editor in `mapdvr.dvr` reads when asked to (`docs/formats/map.md`), and
/// `state.rst`, which no binary names (`docs/formats/rst.md`).
pub const EXPECTED_FILES: &[&str] = &[
    // Executable, driver modules and video mode module.
    "xeno.exe",
    "config.dvr",
    "mapdvr.dvr",
    "mgavideo.dvr",
    "sounddvr.dvr",
    "vgaiff.dvr",
    "vgaintro.dvr",
    "vga240.mod",
    // Tables, sound effects and the 1920-byte layer files.
    "scendef.bin",
    "dsounds.bin",
    "fgarray.kyl",
    "layer1.fgd",
    "layer2.fgd",
    "layer3.fgd",
    "layer4.fgd",
    "layer5.fgd",
    // Drop-sequence images.
    "cavedrop.pcx",
    "xenodrop.pcx",
    // Cutscenes.
    "aftermth.vcl",
    "annihil.vcl",
    "arrival.vcl",
    "detect.vcl",
    "ealogo.vcl",
    "emerge.vcl",
    "grndfall.vcl",
    "nlose.vcl",
    "presents.vcl",
    "return.vcl",
    "slose.vcl",
    "triumph.vcl",
    "war.vcl",
    "win.vcl",
    // Scenarios and training missions.
    "scen00a.usg",
    "scen01a.usg",
    "scen02a.usg",
    "scen03a.usg",
    "scen04a.usg",
    "scen05a.usg",
    "scen06a.usg",
    "scen07a.usg",
    "scen08a.usg",
    "scen09a.usg",
    "scen10a.usg",
    "scen11a.usg",
    "scen12a.usg",
    "ctrain.usg",
    "htrain.usg",
    "ntrain.usg",
    "strain.usg",
    // Screen layouts.
    "humancon.m@l",
    "login.m@l",
    "maproom.m@l",
    "review.m@l",
    "scorpcon.m@l",
    "scoutcon.m@l",
    "sitroom.m@l",
    "trainmen.m@l",
    // Sprite banks and sky strips.
    "msprite0.b@n",
    "msprite1.b@n",
    "msprite2.b@n",
    "msprite3.b@n",
    "msprite4.b@n",
    "msprite5.b@n",
    "sky_a.b@n",
    "sky_b.b@n",
    "sky_c.b@n",
    "sky_d.b@n",
    "sky_e.b@n",
    "sky_f.b@n",
    "sky_g.b@n",
    "sky_h.b@n",
    // Music.
    "bigwin3.ms3",
    "lose.ms3",
    "mainthem.ms3",
    "robot3.ms3",
    "win1.ms3",
    "win2.ms3",
    // Unit models. humanoid, missile, scorpion and scout have no `.m32`.
    "basedome.m00",
    "basedome.m08",
    "basedome.m16",
    "basedome.m32",
    "cloak.m00",
    "cloak.m08",
    "cloak.m16",
    "cloak.m32",
    "explode.m00",
    "explode.m08",
    "explode.m16",
    "explode.m32",
    "humanoid.m00",
    "humanoid.m08",
    "humanoid.m16",
    "laser.m00",
    "laser.m08",
    "laser.m16",
    "laser.m32",
    "missile.m00",
    "missile.m08",
    "missile.m16",
    "netrelay.m00",
    "netrelay.m08",
    "netrelay.m16",
    "netrelay.m32",
    "obst.m00",
    "obst.m08",
    "obst.m16",
    "obst.m32",
    "plasma.m00",
    "plasma.m08",
    "plasma.m16",
    "plasma.m32",
    "scorpion.m00",
    "scorpion.m08",
    "scorpion.m16",
    "scout.m00",
    "scout.m08",
    "scout.m16",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn expected_files_are_lowercase_and_unique() {
        let unique: HashSet<_> = EXPECTED_FILES.iter().collect();
        assert_eq!(unique.len(), EXPECTED_FILES.len());
        for name in EXPECTED_FILES {
            assert_eq!(*name, name.to_lowercase());
        }
    }
}
