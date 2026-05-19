use iced::{Element, Length};
use iced::widget::{button, column, container, row, scrollable, text};
use crate::agents::ActiveAgent;
use crate::db::{AgentTemplate, Project};
use crate::ui::app::Message;

pub fn view_sidebar<'a>(
    projects: &'a [Project],
    agents: &'a [ActiveAgent],
    templates: &'a [AgentTemplate],
    expanded_project: &'a Option<String>,
) -> Element<'a, Message> {
    let mut col = column![
        button(text("📁 Select Directory").size(13))
            .on_press(Message::PickDirectory)
            .width(Length::Fill),
    ]
    .spacing(2)
    .padding(6);

    for project in projects {
        let header = row![
            text(&project.name).size(13).width(Length::Fill),
            button(text("+").size(13))
                .on_press(Message::ToggleTemplateMenu(project.id.clone()))
                .padding([2, 6]),
        ]
        .spacing(4)
        .align_y(iced::alignment::Vertical::Center);

        col = col.push(header);

        if expanded_project.as_deref() == Some(project.id.as_str()) {
            for tmpl in templates {
                let proj_id = project.id.clone();
                let tmpl_id = tmpl.id.clone();
                col = col.push(
                    button(text(format!("  ▶ {}", tmpl.name)).size(12))
                        .on_press(Message::SpawnAgent { project_id: proj_id, template_id: tmpl_id })
                        .width(Length::Fill)
                        .padding([2, 10]),
                );
            }
        }

        for agent in agents.iter().filter(|a| a.project_id == project.id) {
            col = col.push(
                button(text(format!("🤖 {} ({})", agent.template_name, agent.spawned_at)).size(12))
                    .on_press(Message::FocusAgent(agent.id.clone()))
                    .width(Length::Fill)
                    .padding([2, 16]),
            );
        }
    }

    container(scrollable(col))
        .width(250)
        .height(Length::Fill)
        .into()
}
