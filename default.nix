{
  lib,
  rustPlatform,
  pkg-config,
  dbus,
  openssl,
}:
rustPlatform.buildRustPackage {
  pname = "iron_heart";
  version = "0.1.0";

  src = ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
    openssl
  ];

  buildInputs = [
    dbus
    openssl
  ];

  meta = with lib; {
    description = "A BLE Heart Rate Monitor bridge for Social VR, OBS, Data Logging, and more!";
    homepage = "https://github.com/nullstalgia/iron-heart";
    license = licenses.mit;
    maintainers = [];
  };
}
