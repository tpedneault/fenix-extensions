import { createApp } from "./app.js";

const port = Number(process.env.PORT ?? {{port}});

createApp().listen(port, () => {
  console.log(`{{name}} listening on http://localhost:${port}`);
});
