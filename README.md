# Download demo model
```bash
cd front/public
mkdir models && cd models
wget https://cubism.live2d.com/sample-data/bin/tororohijiki/tororo_hijiki.zip
unzip tororo_hijiki.zip
```

# Dev Start
```bash
cd front
npm install
npm run dev
```

```bash
# open another terminal
cargo run
```

open http://127.0.0.1:5173 on web-browser

# Build and run
```bash
cd front
npm run build

cd .. # go back root path
cargo build --release
export RUST_LOG=info
./target/release/vtb_front -d front/dist
```