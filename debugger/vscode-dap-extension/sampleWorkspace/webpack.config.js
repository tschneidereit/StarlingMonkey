import path from 'path';

const config = async () => {
  return {
    stats: "errors-only",
    entry: "./src/main.js",
    experiments: {
      outputModule: true,
    },
    output: {
      path: path.resolve(process.cwd(), "./out"),
      filename: "bundle.js",
      module: true,
      library: {
        type: "module",
      },
    },
    mode: "production",
    optimization: {
    minimize: false,
    },
    devtool: "source-map",
  };
}
export default config