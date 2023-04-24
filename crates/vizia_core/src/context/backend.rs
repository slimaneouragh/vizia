use std::any::Any;
use std::collections::HashSet;

use femtovg::{renderer::OpenGl, Canvas};
use vizia_window::WindowDescription;

use super::EventProxy;
use crate::events::EventManager;
use crate::model::ModelOrView;
use crate::style::SystemFlags;
use crate::{
    cache::CachedData, environment::Environment, layout::geometry_changed, prelude::*,
    style::Style, systems::*,
};
use vizia_id::GenerationalId;

pub use crate::text::cosmic::TextConfig;

#[cfg(feature = "clipboard")]
use copypasta::ClipboardProvider;

/// Context used to integrate vizia with windowing backends such as winit and baseview.
pub struct BackendContext<'a>(pub &'a mut Context, Option<EventManager>);

impl<'a> BackendContext<'a> {
    /// Creates a new instance of a backend context.
    pub fn new(cx: &'a mut Context) -> Self {
        Self(cx, None)
    }

    /// Creates a new instance of a backend context which includes an event manager.
    pub fn new_with_event_manager(cx: &'a mut Context) -> Self {
        Self(cx, Some(EventManager::new()))
    }

    /// Helper function for mutating the state of the root window.
    pub fn mutate_window<W: Any, F: Fn(&mut BackendContext, &W)>(&mut self, f: F) {
        if let Some(window_event_handler) = self.0.views.remove(&Entity::root()) {
            if let Some(window) = window_event_handler.downcast_ref::<W>() {
                f(self, window);
            }

            self.0.views.insert(Entity::root(), window_event_handler);
        }
    }

    pub fn add_window<W: View>(&mut self, window: W) {
        self.0.views.insert(Entity::root(), Box::new(window));
    }

    /// Returns a mutable reference to the style data.
    pub fn style(&mut self) -> &mut Style {
        &mut self.0.style
    }

    /// Returns a mutable reference to the cache of computed properties data.
    pub fn cache(&mut self) -> &mut CachedData {
        &mut self.0.cache
    }

    /// Returns a reference to the keyboard modifiers state.
    pub fn modifiers(&mut self) -> &mut Modifiers {
        &mut self.0.modifiers
    }

    /// Returns the entity id of the currently focused view.
    pub fn focused(&self) -> Entity {
        self.0.focused
    }

    /// The window's size in logical pixels, before
    /// [`user_scale_factor()`][Self::user_scale_factor()] gets applied to it. If this value changed
    /// during a frame then the window will be resized and a [`WindowEvent::GeometryChanged`] will be
    /// emitted.
    pub fn window_size(&mut self) -> &mut WindowSize {
        &mut self.0.window_size
    }

    /// A scale factor used for uniformly scaling the window independently of any HiDPI scaling.
    /// `window_size` gets multplied with this factor to get the actual logical window size. If this
    /// changes during a frame, then the window will be resized at the end of the frame and a
    /// [`WindowEvent::GeometryChanged`] will be emitted. This can be initialized using
    /// [`WindowDescription::user_scale_factor`][crate::WindowDescription::user_scale_factor].
    pub fn user_scale_factor(&mut self) -> f64 {
        self.0.user_scale_factor
    }

    pub fn add_main_window(
        &mut self,
        window_description: &WindowDescription,
        mut canvas: Canvas<OpenGl>,
        dpi_factor: f32,
    ) {
        let physical_width = window_description.inner_size.width as f32 * dpi_factor;
        let physical_height = window_description.inner_size.height as f32 * dpi_factor;

        // Scale factor is set to 1.0 here because scaling is applied prior to rendering
        canvas.set_size(physical_width as u32, physical_height as u32, 1.0);
        canvas.clear_rect(
            0,
            0,
            physical_width as u32,
            physical_height as u32,
            femtovg::Color::rgb(255, 0, 0),
        );

        self.0.style.dpi_factor = dpi_factor as f64;

        self.0.cache.set_width(Entity::root(), physical_width);
        self.0.cache.set_height(Entity::root(), physical_height);

        self.0
            .style
            .width
            .insert(Entity::root(), Units::Pixels(window_description.inner_size.width as f32));
        self.0
            .style
            .height
            .insert(Entity::root(), Units::Pixels(window_description.inner_size.height as f32));

        self.0.style.disabled.insert(Entity::root(), false);

        self.0.canvases.insert(Entity::root(), canvas);
    }

    pub fn environment(&self) -> &Environment {
        self.0.data::<Environment>().unwrap()
    }

    pub fn context(&mut self) -> &mut Context {
        self.0
    }

    pub fn remove_all_children(&mut self) {
        self.0.remove_children(Entity::root());
    }

    pub fn draw(&mut self) {
        draw_system(self.0);
    }

    pub fn load_images(&mut self) {
        image_system(self.0);
    }

    /// Set the current entity. This is useful in user code when you're performing black magic and
    /// want to trick other parts of the code into thinking you're processing some other part of the
    /// tree.
    pub fn set_current(&mut self, e: Entity) {
        self.0.current = e;
    }

    /// Sets the default text configuration to use for text rendering.
    pub fn set_text_config(&mut self, text_config: TextConfig) {
        self.0.text_config = text_config;
    }

    pub fn set_scale_factor(&mut self, scale: f64) {
        self.0.style.dpi_factor = scale;
    }

    /// Temporarily sets the current entity, calls the provided closure, and then resets the current entity back to previous.
    pub fn with_current(&mut self, e: Entity, f: impl FnOnce(&mut Context)) {
        let prev = self.0.current;
        self.0.current = e;
        f(self.0);
        self.0.current = prev;
    }

    /// You should not call this method unless you are writing a windowing backend, in which case
    /// you should consult the existing windowing backends for usage information.
    pub fn set_event_proxy(&mut self, proxy: Box<dyn EventProxy>) {
        if self.0.event_proxy.is_some() {
            panic!("Set the event proxy twice. This should never happen.");
        }

        self.0.event_proxy = Some(proxy);
    }

    /// You should not call this method unless you are writing a windowing backend, in which case
    /// you should consult the existing windowing backends for usage information.
    #[cfg(feature = "clipboard")]
    pub fn set_clipboard_provider(&mut self, clipboard: Box<dyn ClipboardProvider>) {
        self.0.clipboard = clipboard;
    }

    /// Send an event with custom origin and propagation information.
    pub fn send_event(&mut self, event: Event) {
        self.0.event_queue.push_back(event);
    }

    /// Check whether there are any events in the queue waiting for the next event dispatch cycle.
    pub fn has_queued_events(&self) -> bool {
        !self.0.event_queue.is_empty()
    }

    pub fn accesskit_node_classes(&mut self) -> &mut accesskit::NodeClassSet {
        &mut self.style().accesskit_node_classes
    }

    pub fn process_events(&mut self) {
        if let Some(event_manager) = &mut self.1 {
            while event_manager.flush_events(self.0) {}
        }
    }

    /// For each binding or data observer, check if its data has changed, and if so, rerun its
    /// builder/body.
    pub fn process_data_updates(&mut self) {
        let mut observers: HashSet<Entity> = HashSet::new();

        for entity in self.0.tree.into_iter() {
            if let Some(model_data_store) = self.0.data.get_mut(entity) {
                // Determine observers of model data
                for (_, model) in model_data_store.models.iter() {
                    let model = ModelOrView::Model(model.as_ref());

                    for (_, store) in model_data_store.stores.iter_mut() {
                        if store.update(model) {
                            observers.extend(store.observers().iter())
                        }
                    }
                }

                // Determine observers of view data
                for (_, store) in model_data_store.stores.iter_mut() {
                    if let Some(view_handler) = self.0.views.get(&entity) {
                        let view_model = ModelOrView::View(view_handler.as_ref());

                        if store.update(view_model) {
                            observers.extend(store.observers().iter())
                        }
                    }
                }
            }
        }

        for img in self.0.resource_manager.images.values_mut() {
            if img.dirty {
                observers.extend(img.observers.iter());
                img.dirty = false;
            }
        }

        let ordered_observers =
            self.0.tree.into_iter().filter(|ent| observers.contains(ent)).collect::<Vec<_>>();

        // Update observers in tree order
        for observer in ordered_observers.into_iter() {
            if !self.0.entity_manager.is_alive(observer) {
                continue;
            }

            if let Some(mut binding) = self.0.bindings.remove(&observer) {
                let prev = self.0.current;
                self.0.current = observer;
                binding.update(self.0);
                self.0.current = prev;
                self.0.bindings.insert(observer, binding);
            }
        }
    }

    pub fn process_tree_updates(&mut self, process: impl Fn(&Vec<accesskit::TreeUpdate>)) {
        accessibility_system(self.0);

        (process)(&self.0.tree_updates);

        self.0.tree_updates.clear();
    }

    pub fn process_style_updates(&mut self) {
        // Apply any inline style inheritance.
        inline_inheritance_system(self.0);

        style_system(self.0);

        shared_inheritance_system(self.0);

        // Load any unloaded images and remove unused images.
        image_system(self.0);
    }

    // Returns true if animations are playing
    pub fn process_animations(&mut self) -> bool {
        animation_system(self.0)
    }

    /// Massages the style system until everything is coherent
    pub fn process_visual_updates(&mut self) {
        // Perform layout.
        layout_system(self.0);

        // Emit any geometry changed events.
        geometry_changed(self.0);
    }

    pub fn emit_origin<M: Send + Any>(&mut self, message: M) {
        self.0.event_queue.push_back(
            Event::new(message)
                .target(self.0.current)
                .origin(Entity::root())
                .propagate(Propagation::Up),
        );
    }

    pub fn needs_refresh(&mut self) {
        self.0.style.system_flags = SystemFlags::all();
    }
}
