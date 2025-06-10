import { fields } from "@keystatic/core";

export const opengraph = fields.object(
  {
    title: fields.text({
      label: "Title",
      validation: {
        isRequired: true,
      },
    }),
    description: fields.text({
      label: "Description",
      multiline: true,
      description: "Short description, mostly for SEO",
    }),
    type: fields.select({
      label: "Type",
      options: [
        { label: "Website", value: "website" },
        { label: "Article", value: "article" },
      ],
      defaultValue: "website",
      description: "The type of Open Graph object (e.g., website, article).",
    }),
    image: fields.image({
      label: "Opengraph Image",
      directory: "src/assets/images/open-graph",
      publicPath: "/src/assets/images/open-graph/",
    }),
    publishedTime: fields.date({
      label: "Published Time",
      description: "The date and time when the content was published.",
    }),
    modifiedTime: fields.date({
      label: "Modified Time",
      description: "The date and time when the content was last modified.",
    }),
    author: fields.text({
      label: "Author",
      description: "The name of the author of the content.",
    }),
    primaryCategory: fields.select({
      label: "Primary Category",
      options: [
        { label: "News", value: "news" },
        { label: "Blog", value: "blog" },
        { label: "Tutorial", value: "tutorial" },
        { label: "Documentation", value: "documentation" },
        { label: "About", value: "about" },
        { label: "Other", value: "other" },
      ],
      defaultValue: "about",
      description: "The primary category of the content.",
    }),
  },
  {
    label: "Open Graph",
    description: "Open Graph metadata for social media sharing",
  }
);
