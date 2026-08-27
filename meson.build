project(
  'Elysiae',
  'rust',
  version: '1.0.0',
  license: 'BSD-3-Clause',
  meson_version: '>= 1.11.0',
  default_options: [
    'warning_level=2',
    'layout=mirror',
  ],
)

gnome = import('gnome')

base_id = 'app.elysiae.Elysiae'

is_devel = get_option('profile') == 'development'
if is_devel
  profile = 'Devel'
  application_id = '@0@.@1@'.format(base_id, profile)
else
  profile = ''
  application_id = base_id
endif

bindir = get_option('prefix') / get_option('bindir')
datadir = get_option('prefix') / get_option('datadir')
pkgdatadir = datadir / meson.project_name()

subdir('data')
subdir('src')

# Post-install scripts are only useful when the files they refresh are
# actually installed. Skip them for pure local builds that never run
# `meson install` (see `skip-post-install-scripts` in meson.options).
if not get_option('skip-post-install-scripts')
  gnome.post_install(
    gtk_update_icon_cache: true,
    glib_compile_schemas: true,
    update_desktop_database: true,
  )
endif

summary(
  {
    'Application ID': application_id,
    'Profile': get_option('profile'),
    'Prefix': get_option('prefix'),
    'Build type': get_option('buildtype'),
    'Resources': resources_path,
    'Cargo': cargo.full_path(),
  },
  section: 'Build',
)