import express from "express";

/** The app, without a port: the server listens, the tests call it. */
export function createApp() {
  const app = express();
  app.use(express.json());

  app.get("/health", (_req, res) => {
    res.json({ ok: true });
  });

  app.post("/echo", (req, res) => {
    res.status(201).json(req.body);
  });

  return app;
}
