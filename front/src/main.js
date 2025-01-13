import { loadOml2d } from 'oh-my-live2d';

let url = new URL(window.location.href);
let actor = url.searchParams.get('actor') || 'default';
console.log(`actor: ${actor}`);

const live2dModels = window.live2dModels;

var oml2dObjs = {}
window.oml2dObjs = oml2dObjs;

function load_models(actor) {
  var model = live2dModels[actor]
  model.tips = {
    messageLine: 10
  }
  var oml2dObj = loadOml2d(model);
  oml2dObjs[actor] = oml2dObj
}

function clear_all_tips() {
  for (var key in oml2dObjs) {
    oml2dObjs[key].clearTips()
  }
}

var audioContext;

var async_queue = {
  data: [],
  resolve: undefined
}

function put_item(data) {
  async_queue.data.push(data)
  if (async_queue.resolve) {
    async_queue.resolve()
  }
}

async function get_item() {
  if (async_queue.data.length === 0) {
    let r = new Promise((resolve, _) => {
      async_queue.resolve = resolve
    })
    await r
  }
  return async_queue.data.shift()
}


async function wav_loop() {
  while (true) {
    let ws_data = await get_item()
    if (ws_data instanceof Blob) {
      try {
        await playWav(ws_data)
        clear_all_tips()
      } catch (e) {
        console.error(e)
      }
    } else {
      let data = JSON.parse(ws_data)
      if (data.motion) {
        showMotion(data.motion)
      }
      if (data.say) {
        say(data.vtb_name, data.say, 600 * 1000)
      }
    }
  }
}

wav_loop()

var ws = undefined;

async function playWav(wavData) {
  if (!audioContext) {
    audioContext = new (window.AudioContext || window.webkitAudioContext)()
  }
  let bufferData = await wavData.arrayBuffer()
  let audioBuffer = await audioContext.decodeAudioData(bufferData)
  const bufferSource = audioContext.createBufferSource()
  bufferSource.buffer = audioBuffer
  bufferSource.connect(audioContext.destination)
  bufferSource.start(0)
  let p = new Promise((resolve, _reject) => {
    bufferSource.onended = () => {
      resolve()
    }
  })
  await p
}

function display(text) {
  var div = document.getElementById("app");

  div.style.fontSize = "16px"; // 设置字体大小
  div.style.color = "white";     // 设置字体颜色
  var textNode = document.createTextNode(text);
  div.appendChild(textNode);
  div.appendChild(document.createElement("br"));
}

function clear_display() {
  var div = document.getElementById("app");
  div.innerHTML = "";
}

var connecting = false;

function connect_backend() {
  if (connecting) {
    return
  }
  connecting = true
  display("Connecting to backend...")
  let ws_url = `/ws/${actor}`

  try {
    ws = new WebSocket(ws_url)
  } catch (e) {
    var location = window.location;
    var protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    var newUrl = protocol + '//' + location.hostname;
    if (location.port) {
      newUrl += ':' + location.port;
    }
    newUrl += ws_url;
    display(newUrl)
    ws = new WebSocket(newUrl)
  }
  // ws = new WebSocket(ws_url)
  ws.onmessage = (event) => {
    put_item(event.data)
  }
  ws.onerror = (event) => {
    connecting = false
    console.error(event)
    display(`Failed to connect to backend ${event}`)
  }
  ws.onclose = () => {
    connecting = false
    clear_display()
    say("default", "Connecting to backend...")
    setTimeout(() => {
      connect_backend()
    }, 5000)
  }
  ws.onopen = () => {
    connecting = false
    clear_display()
    display("Connected to backend")
  }

}

function say(vtb_name, message, duartion = 3000) {
  try {
    oml2dObjs[vtb_name].tipsMessage(message, duartion)
  } catch (e) {
    console.error(e)
  }
}

function showMotion(vtb_name, motionGroup) {
  try {
    oml2dObjs[vtb_name].models.playMotion(motionGroup)
  } catch (e) {
    console.error(e)
  }
}

load_models('default')
load_models('black_cat')

connect_backend()