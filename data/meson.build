resources = gnome.compile_resources(
  'resources',
  'resources.gresource.xml',
  gresource_bundle: true,
  source_dir: [meson.current_source_dir(), meson.current_build_dir()],
  install: true,
  install_dir: pkgdatadir,
)

resources_path = meson.current_build_dir() / 'resources.gresource'

gschema_dir = datadir / 'glib-2.0' / 'schemas'
install_data('app.elysiae.Elysiae.gschema.xml', install_dir: gschema_dir)