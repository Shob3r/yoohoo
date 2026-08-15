use gtk::CompositeTemplate;
use gtk::glib;
use gtk::subclass::prelude::*;

#[derive(Default, CompositeTemplate)]
#[template(resource = "/app/elysiae/Elysiae/window.ui")]
pub struct Window {
    #[template_child]
    pub label_widget: TemplateChild<gtk::Label>,
}

#[glib::object_subclass]
impl ObjectSubclass for Window {
    const NAME: &'static str = "Elysiae";
    type Type = super::Window;
    type ParentType = gtk::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.bind_template_callbacks();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

#[gtk::template_callbacks]
impl Window {
    #[template_callback]
    fn on_button_clicked(&self, _button: &gtk::Button) {
        self.label_widget.set_label("Clicked!");
    }
}

impl ObjectImpl for Window {}
impl WidgetImpl for Window {}
impl WindowImpl for Window {}
impl ApplicationWindowImpl for Window {}
