window.live2dModels = {
    'default': {
        sayHello: false,
        models: [
            {
                path: 'https://cdn.jsdelivr.net/gh/Eikanya/Live2d-model/Live2D/Senko_Normals/senko.model3.json',
                // path: '/live2d_models/senko.model3.json',
                scale: 0.2,
                position: [0, 120],
                stageStyle: {
                    left: '600px',
                    height: 700,
                    width: 600
                }
            }
        ]
    },
    'black_cat': {
        sayHello: false,
        models: [
            {
                path: "https://model.oml2d.com/cat-black/model.json",
                scale: 0.2,
                position: [0, 150],
                stageStyle: {
                    height: 700,
                    width: 600
                }
            }
        ]
    }
}