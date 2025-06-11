import { config, fields, collection } from "@keystatic/core";
import { homepage } from "@lib/keystatic/singletons/homepage";

export default config({
  storage: {
    kind: "local",
  },
  locale: "en-US",
  collections: {
    posts: collection({
      label: "Posts",
      slugField: "title",
      path: "src/content/posts/*",
      format: { contentField: "content" },
      schema: {
        title: fields.slug({ name: { label: "Title" } }),
        content: fields.markdoc({
          label: "Content",
        }),
      },
    }),
  },
  singletons: {
    homepage,
  },
});
