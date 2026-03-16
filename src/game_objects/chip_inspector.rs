use std::any::Any;

use macroquad::{
    color::BLACK,
    miniquad::window::screen_size,
    shapes::{draw_rectangle, draw_rectangle_lines},
    ui::{Ui, root_ui},
};

use crate::{
    Resource,
    game_objects::{GameObject, GameObjectData, GameObjects, ObjectId, TypeMap},
    simulation::{Chip, ChipId, ChipInstance, Simulation},
};

const INSPECTOR_PANEL_OFFSET: f32 = 50.;

fn draw_inspector_panel() {
    let (width, height) = screen_size();
    draw_rectangle(
        INSPECTOR_PANEL_OFFSET,
        INSPECTOR_PANEL_OFFSET,
        width - 2. * INSPECTOR_PANEL_OFFSET,
        width - 2. * INSPECTOR_PANEL_OFFSET,
        BLACK.with_alpha(0.7),
    );
    draw_rectangle_lines(
        INSPECTOR_PANEL_OFFSET,
        INSPECTOR_PANEL_OFFSET,
        width - 2. * INSPECTOR_PANEL_OFFSET,
        height - 2. * INSPECTOR_PANEL_OFFSET,
        2.,
        BLACK.with_alpha(0.9),
    );
}

pub struct PanelData {
    render: Box<dyn FnMut(&mut dyn Any) + 'static>,
    apply: Box<dyn FnOnce(Box<dyn Any>, &mut GameObjectData, &mut ChipInstance) + 'static>,
    state: Box<dyn Any>,
}

impl PanelData {
    pub fn new<O, C, S, R, A>(mut render: R, apply: A, value: S) -> Self
    where
        O: GameObject + 'static,
        C: Chip + 'static,
        S: 'static,
        R: FnMut(&mut Ui, &mut S) + 'static,
        A: FnOnce(S, &mut O, &mut C) + 'static,
    {
        Self {
            state: Box::new(value),
            render: Box::new(move |data: &mut dyn Any| {
                let data = data.downcast_mut::<S>().unwrap();
                (render)(&mut *root_ui(), data);
            }),
            apply: Box::new({
                move |data, object, chip| {
                    let data = *data.downcast::<S>().unwrap();
                    let object = object.downcast_mut().unwrap();
                    let chip = chip.downcast_mut().unwrap();

                    apply(data, object, chip)
                }
            }),
        }
    }

    pub fn render(&mut self) {
        (self.render)(self.state.as_mut())
    }

    pub fn apply(self, object: &mut GameObjectData, chip: &mut ChipInstance) {
        (self.apply)(self.state, object, chip)
    }
}

/// On double click, open a chip inspector/editor menu for the selected chip.
pub struct OpenInspectorPanel(ChipId, ObjectId, Option<PanelData>);

impl Resource for OpenInspectorPanel {
    // TODO: update signature and callsite
    fn render(&mut self, _: &mut TypeMap) {
        draw_inspector_panel();

        self.2.as_mut().unwrap().render();
    }
}

impl OpenInspectorPanel {
    pub fn new(chip: ChipId, object: ObjectId, data: PanelData) -> Self {
        Self(chip, object, Some(data))
    }

    pub fn apply(&mut self, objects: &mut GameObjects, simulation: &mut Simulation) {
        let object = objects.get_mut(self.1).unwrap();
        let chip = simulation.chips.get_mut(self.0).unwrap();

        self.2.take().unwrap().apply(object, chip);
    }
}
