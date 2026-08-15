blueprint_compiler = find_program('blueprint-compiler')

blueprint_sources = files('ui/window.blp')

ui_files = []
foreach blp : blueprint_sources
    ui_file = custom_target(
        blp.full_path().underscorify(),
        input: blp,
        output: '@BASENAME@.ui',
        command: [
            blueprint_compiler,
            'compile',
            '--output', '@OUTPUT@',
            '@INPUT@',
        ],
    )
    ui_files += ui_file
endforeach

resources = gnome.compile_resources(
    'resources',
    'resources.gresource.xml',
    gresource_bundle: true,
    source_dir: [meson.current_source_dir(), meson.current_build_dir()],
    dependencies: ui_files,
    install: true,
    install_dir: pkgdatadir,
)
