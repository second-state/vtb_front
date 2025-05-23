import { Live2DModel, logger } from 'pixi-live2d-display';
import * as PIXI from 'pixi.js';

let url = new URL(window.location.href);
let actor = url.searchParams.get('actor') || 'default';
let interaction = url.searchParams.get('interaction') || 'auto';
console.log(`actor: ${actor}`);

const live2dScene = window.live2dScene;

window.live2d_models = {};

var ws = undefined;

var analyser = undefined;

var audioContext;

var async_queue = {
  data: [],
  resolve: undefined
}

function cleanQueue() {
  async_queue.data = []
}

function putItem(data) {
  if (data instanceof Blob) {
    async_queue.data.push(data)
  } else {
    let data_json = JSON.parse(data)
    if (data_json['type'] == 'UpdateTitle') {
      document.getElementById('title').innerText = data_json['title']
    } else {
      async_queue.data.push(data_json)
    }
  }
  if (async_queue.resolve) {
    async_queue.resolve()
  }
}

async function getItem() {
  if (async_queue.data.length === 0) {
    let r = new Promise((resolve, _) => {
      async_queue.resolve = resolve
    })
    await r
  }
  return async_queue.data.shift()
}


async function wavLoop() {
  let waker = null;
  while (true) {
    let ws_data = await getItem()
    if (ws_data instanceof Blob) {
      try {
        await playWav(ws_data)
      } catch (e) {
        console.error(e)
      }
      if (waker !== null) {
        if (ws !== undefined && ws.readyState === WebSocket.OPEN) {
          ws.send(waker + '')
          waker = null
        }
      }
    } else {
      let data = ws_data
      switch (data['type']) {
        case 'ChangeScene':
          await load_all_models(data['index'])
          break
        case 'Speech':
          say(data['vtb_name'], data['message'])
          if (data['motion'] !== '') {
            showMotion(data['vtb_name'], data['motion'])
          }
          waker = data['waker']
          break
      }
    }
  }
}

wavLoop()


function lipSync(model_name, value) {
  let model = window.live2d_models[model_name]
  if (model && model.internalModel.motionManager.lipSyncIds.length > 0) {
    var lip_id = model.internalModel.motionManager.lipSyncIds[0]
    model.internalModel.coreModel.setParameterValueById(lip_id, value, 0.8)
  }
}

function getAverageVolume(array) {
  let values = 0;
  let average;

  const length = array.length;

  for (let i = 0; i < length; i++) {
    values += array[i];
  }

  average = values / length;
  return Math.min(average, 50.0) / 50.0;
}

var frequencyData;

var speaker = ''

function closeMouth() {
  lipSync(speaker, 0)
  setTimeout(updateVolume, 90)
}

function updateVolume() {
  if (analyser == undefined) {
    return
  }

  analyser.getByteFrequencyData(frequencyData);

  const volume = getAverageVolume(frequencyData);
  if (volume > 0.4) {
    lipSync(speaker, volume)
  }

  setTimeout(closeMouth, 90)
}

async function playWav(wavData) {
  if (!audioContext) {
    audioContext = new (window.AudioContext || window.webkitAudioContext)()
  }
  analyser = audioContext.createAnalyser()
  analyser.fftSize = 256;
  frequencyData = new Uint8Array(analyser.frequencyBinCount);

  let bufferData = await wavData.arrayBuffer()
  let audioBuffer = await audioContext.decodeAudioData(bufferData)
  const bufferSource = audioContext.createBufferSource()
  bufferSource.buffer = audioBuffer
  bufferSource.connect(analyser)
  analyser.connect(audioContext.destination)
  bufferSource.start(0)
  updateVolume()
  let p = new Promise((resolve, _reject) => {
    bufferSource.onended = () => {
      analyser = undefined
      lipSync(speaker, 0)
      resolve()
    }
  })
  await p
}

var connecting = false;

function connectBackend() {
  if (connecting) {
    return
  }
  connecting = true
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
    ws = new WebSocket(newUrl)
  }
  // ws = new WebSocket(ws_url)
  ws.onmessage = (event) => {
    putItem(event.data)
  }
  ws.onerror = (event) => {
    connecting = false
    console.error(event)
  }
  ws.onclose = () => {
    connecting = false
    cleanQueue()
    say("Log", "Connecting to backend...")
    setTimeout(() => {
      connectBackend()
    }, 5000)
  }
  ws.onopen = () => {
    connecting = false
    say("Log", "Connect Ok")
  }
}

const messsages = []

function say(vtb_name, message) {
  speaker = vtb_name
  // messsages.push({ vtb_name, message })
  // if (messsages.length > 5) {
  //   messsages.shift()
  // }
  let div = document.getElementById('messages');
  // let p = '';
  // for (var m in messsages) {
  //   p += `${messsages[m].vtb_name}> ${messsages[m].message}\n`;
  // }
  let p = `${vtb_name}> ${message}`;

  div.innerText = p;
  div.scrollTop = div.scrollHeight;
}

function showMotion(vtb_name, motion) {
  let model = window.live2d_models[vtb_name];
  if (model) {
    model.internalModel.motionManager.startRandomMotion(motion, 3);
  }
}

connectBackend()

function draggable(model) {
  model.buttonMode = true;
  model.on("pointerdown", (e) => {
    console.log("pointerdown");
    model.dragging = true;
    model._pointerX = e.data.global.x - model.x;
    model._pointerY = e.data.global.y - model.y;
  });
  model.on("pointermove", (e) => {
    if (model.dragging) {
      model.position.x = e.data.global.x - model._pointerX;
      model.position.y = e.data.global.y - model._pointerY;
    }
  });
  model.on("pointerupoutside", () => {
    model.dragging = false
    console.log(`model position: ${model.x}, ${model.y}`);
  });
  model.on("pointerup", () => {
    model.dragging = false
    console.log(`model position: ${model.x}, ${model.y}`);
  });
}

// internalModel.motionManager.lipSyncIds
window.PIXI = PIXI;

var current_index = -1;

async function load_all_models(index) {
  if (window.pixi_app == undefined) {
    window.pixi_app = new PIXI.Application({
      view: document.getElementById("canvas"),
      autoStart: true,
      resizeTo: window,
      backgroundAlpha: 0.5,
    });
  }

  if (live2dScene[index] == undefined) {
    return
  }

  if (current_index == index) {
    return
  }

  current_index = index;

  let backgroud_image = live2dScene[index].background;
  document.body.style.backgroundImage = `url(${backgroud_image})`;

  let live2dModels = live2dScene[index].models;
  let app = window.pixi_app;
  app.stage.removeChildren()
  window.live2d_models = {}


  for (var key in live2dModels) {
    var model_config = live2dModels[key].model;
    var model = await Live2DModel.from(model_config.path);
    app.stage.addChild(model);
    model.scale.set(model_config.scale);
    model.eventMode = interaction;

    model.x = model_config.position[0];
    model.y = model_config.position[1];
    for (var p in model_config.parameter) {
      model.internalModel.coreModel.setParameterValueById(p, model_config.parameter[p]);
    }
    window.live2d_models[key] = model;
    draggable(model)
  }

};

window.load_all_models = load_all_models;

document.addEventListener("DOMContentLoaded", async () => {
  load_all_models(0)
})