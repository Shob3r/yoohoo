use gtk::glib;

mod imp {
    use gtk::CompositeTemplate;
    use gtk::glib;
    use gtk::subclass::prelude::*;

    #[derive(CompositeTemplate, Default)]
    #[template(resource = "/app/elysiae/Elysiae/ui/button.ui")]
    pub struct Button;

    #[glib::object_subclass]
    impl ObjectSubclass for Button {
        const NAME: &'static str = "ElysiaeButton";
        type Type = super::Button;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Button {}
    impl WidgetImpl for Button {}
    impl BoxImpl for Button {}
}

glib::wrapper! {
    pub struct Button(ObjectSubclass<imp::Button>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl Button {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }
}

impl Default for Button {
    fn default() -> Self {
        Self::new()
    }
}
