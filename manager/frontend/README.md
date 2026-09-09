# Zone Manager Frontend

This is the React frontend for the Zone Manager console.

## Available Scripts

In the project directory, you can run:

### `bun start`

Runs the app in the development mode.\
Open [http://localhost:3000](http://localhost:3000) to view it in the browser.

### `bun test`

Launches the test runner.\
Use `bun test --watch` for watch mode.

### `bun run build`

Builds the app for production to the `dist` folder.\
It correctly bundles React in production mode and optimizes the build for the best performance.

### `bun run lint`

Runs the linter (Biome) on the source code.

### `bun run test:e2e`

Runs end-to-end tests with Playwright against mocked API responses (`e2e/`).

### `bun run test:live`

Runs the live suite (`live/`) against a real server. Nothing in it mocks a
route, forges a token, or answers for the API, so a passing test has seen the
console render what the server actually sent. It needs the rig
`make live-verify` brings up — run that instead unless the rig is already
running, in which case set `ZONE_LIVE_STATE`, `ZONE_COMFY_FIXTURES` and
`ZONE_TRAIN_FIXTURES` to what the script printed.

## Learn More

- [Bun Documentation](https://bun.sh/docs)
- [Vite Documentation](https://vitejs.dev/)
- [React Documentation](https://reactjs.org/)
