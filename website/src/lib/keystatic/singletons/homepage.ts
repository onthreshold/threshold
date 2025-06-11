import { fields, singleton } from "@keystatic/core";
import { action } from "@lib/keystatic/shared/action";
import { opengraph } from "@lib/keystatic/shared/opengraph";

export const homepage = singleton({
  label: "Homepage",
  path: "src/content/singles/homepage/",
  schema: {
    openGraph: opengraph,
    actions: fields.array(action, {
      label: "Actions",
      description: "A list of action buttons for the homepage.",
    }),
    waitlist: fields.object(
      {
        label: fields.text({
          label: "Waitlist Label",
          description: "The label for the waitlist section on the homepage.",
          defaultValue: "join waitlist",
        }),
        description: fields.text({
          label: "Waitlist Description",
          description: "A brief description for the waitlist section.",
          defaultValue:
            "Get updates on our progress and be the first to know when we launch.",
        }),
        inputPlaceholder: fields.text({
          label: "Waitlist Input Placeholder",
          description: "Placeholder text for the waitlist input field.",
          defaultValue: "your@email.com",
        }),
        buttonText: fields.text({
          label: "Waitlist Button Text",
          description: "The text for the waitlist button on the homepage.",
          defaultValue: "join waitlist",
        }),
        numberSuffix: fields.text({
          label: "Waitlist Number Suffix",
          description:
            "The suffix for the waitlist number displayed on the homepage.",
          defaultValue: "people waiting",
        }),
      },
      {
        label: "Waitlist Section",
        description: "Configuration for the homepage waitlist section.",
      }
    ),
    roadmap: fields.object(
      {
        label: fields.text({
          label: "Roadmap Title",
          description: "The title for the homepage roadmap section.",
          defaultValue: "roadmap",
        }),
        items: fields.array(
          fields.object(
            {
              label: fields.text({
                label: "Label",
                description: "The label for the roadmap item.",
              }),
              description: fields.text({
                label: "Description",
                description: "A brief description of the roadmap item.",
              }),
              status: fields.select({
                label: "Status",
                description: "The current status of the roadmap item.",
                options: [
                  { value: "not started", label: "Not Started" },
                  { value: "in progress", label: "In Progress" },
                  { value: "completed", label: "Completed" },
                ],
                defaultValue: "not started",
              }),
            },
            {
              label: "Roadmap Item",
              description: "An item in the homepage roadmap section.",
            }
          ),
          {
            label: "Roadmap Items",
            description: "A list of items for the homepage roadmap section.",
          }
        ),
      },
      {
        label: "Roadmap Section",
        description: "Configuration for the homepage roadmap section.",
      }
    ),
  },
});
