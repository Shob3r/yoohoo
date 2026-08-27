use gtk::{AccessibleRole::{Row, TextBox}, Application, Box as GtkBox, Button, HeaderBar, ListBox, Window, prelude::{BoxExt, ButtonExt}};

pub fn build_window(app: &Application) -> Window {
    let header = HeaderBar::new();

    let btn = Button::builder().label("Click ME!").build();

    let content = GtkBox::new(gtk::Orientation::Vertical, 0);

    content.append(&btn);

    Window::builder()
        .application(app)
        .title("Elysiae")
        .default_height(800)
        .default_width(1200)
        .build()
}
