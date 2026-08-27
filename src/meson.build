cargo = find_program('cargo', native: true, required: true)

base_cargo_options = [
  '--manifest-path', meson.project_source_root() / 'Cargo.toml',
  '--target-dir', meson.project_build_root() / 'target',
]

if is_devel
  rust_target = 'debug'
else
  base_cargo_options += ['--release']
  rust_target = 'release'
endif

fmt_options = ['--manifest-path', meson.project_source_root() / 'Cargo.toml']

if get_option('prefix') == '/app'
  resources_file = pkgdatadir / 'resources.gresource'
else
  resources_file = resources_path
endif

cargo_env = {
  'CARGO_HOME': meson.project_build_root() / 'cargo-home',
  'APP_ID': application_id,
  'RESOURCES_FILE': resources_file,
}

binary = meson.project_build_root() / 'target' / rust_target / meson.project_name()

custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: meson.project_name(),
  depends: resources,
  console: true,
  install: true,
  install_dir: bindir,
  env: cargo_env,
  command: [
    cargo, 'build', base_cargo_options, '&&', 'cp', binary, '@OUTPUT@',
  ],
)

run_target(
  'run',
  command: [cargo, 'run', base_cargo_options, '--bin', meson.project_name()],
  depends: resources,
  env: cargo_env,
)

run_target('fmt',          command: [cargo, 'fmt',    fmt_options])
run_target('cargo-clippy', command: [cargo, 'clippy', base_cargo_options])
run_target('cargo-test',  command: [cargo, 'test',   base_cargo_options])
