import { fields, singleton } from "@keystatic/core";
import { action } from "@lib/keystatic/shared/action";
import { opengraph } from "@lib/keystatic/shared/opengraph";

export const homepage = singleton({
  label: "Homepage",
  path: "src/content/singles/homepage/",
  schema: {
    openGraph: opengraph,
    actions: fields.array(action),
  },
});
