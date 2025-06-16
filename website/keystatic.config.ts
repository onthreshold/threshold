import { config } from "@keystatic/core";
import { blogPosts } from "@lib/keystatic/collections/blog-posts";
import { blogCategories } from "@lib/keystatic/collections/blog-categories";
import { homepage, logo } from "@lib/keystatic/singletons/homepage";

export default config({
  storage: {
    kind: "local",
  },
  locale: "en-US",
  collections: {
    blogCategories,
    blogPosts,
  },
  singletons: {
    logo,
    homepage,
  },
});
