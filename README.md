
# 🌐 USTE: Universal-Spatial Temporal Engine

**USTE** is a high-performance, GPU-accelerated graph database designed for real-time spatial-temporal data processing. Leveraging TensorFlow for computation and TimescaleDB for storage, USTE enables efficient handling of complex data structures with spatial and temporal dimensions.

---

## 🚀 Features

- **GPU Acceleration**: Utilizes TensorFlow to perform rapid computations on graph data.
- **Spatial-Temporal Data Handling**: Supports data with both spatial and temporal attributes.
- **Graph Neural Network Integration**: Compatible with GNN architectures for advanced analytics.
- **Scalable Storage**: Employs TimescaleDB for efficient time-series data storage.
- **API Access**: Provides FastAPI endpoints for seamless integration with applications.

---

## 🛠️ Installation

### Clone the Repository:

```bash
git clone https://github.com/yourusername/uste.git
cd uste
```

### Set Up the Environment:

Create and activate a new Conda environment:

```bash
conda create -n uste_env python=3.12 -y
conda activate uste_env
```

### Install Dependencies:

```bash
pip install -r requirements.txt
```

### Configure TimescaleDB:

Ensure TimescaleDB is installed and running. Update the database configuration in `.env` file.

---

## 📦 Usage

### Start the FastAPI Server:

```bash
uvicorn main:app --reload
```

### API Endpoints:

- `POST /nodes/`: Add a new node with spatial-temporal data.
- `POST /edges/`: Create a connection between nodes.
- `GET /graph/`: Retrieve the current state of the graph.
- `GET /compute/`: Perform computations on the graph data.

---

## 📂 Project Structure

```
uste/
├── app/
│   ├── main.py
│   ├── models.py
│   ├── database.py
│   └── utils.py
├── tests/
│   └── test_app.py
├── requirements.txt
├── .env
└── README.md
```

---

## 🧪 Testing

Run the test suite using:

```bash
pytest tests/
```

---

## 🤝 Contributing

Contributions are welcome! Please fork the repository and submit a pull request for review.

---

## 📄 License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

---


---

## 🖼️ Images

To attach images, create a folder named `images` in your repository and upload your images there. Reference them in your README.md as follows:

```markdown
![Alt text for your image](images/your-image.png)
```



