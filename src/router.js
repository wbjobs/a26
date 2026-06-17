import RecorderPage from "./pages/RecorderPage.svelte";
import LibraryPage from "./pages/LibraryPage.svelte";
import PlaybackPage from "./pages/PlaybackPage.svelte";

export const routes = [
  {
    name: "/",
    component: RecorderPage,
  },
  {
    name: "library",
    component: LibraryPage,
  },
  {
    name: "playback/:id",
    component: PlaybackPage,
  },
];
