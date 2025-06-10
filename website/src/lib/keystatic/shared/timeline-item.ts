import { fields } from "@keystatic/core";

export const timelineItem = fields.object(
  {
    title: fields.text({
      label: "Title",
      description:
        "A brief, descriptive title for this timeline event or milestone",
      validation: {
        isRequired: true,
      },
    }),
    description: fields.text({
      label: "Description",
      multiline: true,
      description: "Description of the timeline event",
    }),
    status: fields.select({
      label: "Status",
      options: [
        { label: "Completed", value: "completed" },
        { label: "In Progress", value: "in-progress" },
        { label: "Planned", value: "planned" },
        { label: "Cancelled", value: "cancelled" },
      ],
      defaultValue: "planned",
      description: "Current status of the timeline item",
    }),
  },
  {
    label: "Timeline Item",
    description: "A single item in the timeline",
  }
);
