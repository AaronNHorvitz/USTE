# USTE: MVP Build Plan

This plan outlines the granular steps to build the Minimum Viable Product (MVP) for the Universal-Spatial Temporal Engine (USTE). Each task should be completed and tested individually.

## Phase 1: Project & Environment Setup

**Task 1.1: Initialize Project Directory Structure**
* **Goal**: Create the initial root directory and essential top-level subdirectories as defined in the architecture.
* **Action(s)**:
    1.  Create the root project directory named `USTE`.
    2.  Inside `USTE/`, create the following directories: `data/`, `images/`, `notebooks/`, `sandbox/`, `src/`, `tests/`.
    3.  Inside `USTE/data/`, create `arango/` and `timescale/`.
    4.  Inside `USTE/src/`, create `app/`, `database/`, `embeddings/`.
* **File(s) to Modify/Create**: None (directory creation).
* **Verification**:
    * Run `ls -R USTE/` (or equivalent) to verify the directory structure matches:
        ```
        USTE/
        ├── data/
        │   ├── arango/
        │   └── timescale/
        ├── images/
        ├── notebooks/
        ├── sandbox/
        ├── src/
        │   ├── app/
        │   ├── database/
        │   └── embeddings/
        └── tests/
        ```

**Task 1.2: Create Initial `README.md`**
* **Goal**: Create a placeholder `README.md` file.
* **Action(s)**:
    1.  Create a file named `README.md` in the `USTE/` directory.
    2.  Add the following content: `# USTE: Universal-Spatial Temporal Engine`.
* **File(s) to Modify/Create**: `USTE/README.md`
* **Verification**:
    * Check that `USTE/README.md` exists and contains the specified title.

**Task 1.3: Create Initial `requirements.txt`**
* **Goal**: Create an empty `requirements.txt` file. This will be populated in later steps.
* **Action(s)**:
    1.  Create an empty file named `requirements.txt` in the `USTE/` directory.
* **File(s) to Modify/Create**: `USTE/requirements.txt`
* **Verification**:
    * Check that `USTE/requirements.txt` exists and is empty.

**Task 1.4: Create `.gitignore` file**
* **Goal**: Add a basic `.gitignore` file to exclude common Python and environment files.
* **Action(s)**:
    1. Create a file named `.gitignore` in the `USTE/` directory.
    2. Add the following content:
        ```gitignore
        # Byte-compiled / optimized / DLL files
        __pycache__/
        *.py[cod]
        *$py.class

        # C extensions
        *.so

        # Distribution / packaging
        .Python
        build/
        develop-eggs/
        dist/
        downloads/
        eggs/
        .eggs/
        lib/
        lib64/
        parts/
        sdist/
        var/
        wheels/
        *.egg-info/
        .installed.cfg
        *.egg
        MANIFEST

        # PyInstaller
        #  Usually these files are written by a python script from a template
        #  before PyInstaller builds the exe, so as to inject date/other infos into it.
        *.manifest
        *.spec

        # Installer logs
        pip-log.txt
        pip-delete-this-directory.txt

        # Unit test / coverage reports
        htmlcov/
        .tox/
        .nox/
        .coverage
        .coverage.*
        .cache
        nosetests.xml
        coverage.xml
        *.cover
        .hypothesis/
        .pytest_cache/

        # Environments
        .env
        .venv
        env/
        venv/
        ENV/
        env.bak/
        venv.bak/

        # Docker
        .dockerignore
        docker-compose.override.yml

        # Data files (if they are too large or sensitive for git)
        # data/arango/*
        # data/timescale/*
        # !data/arango/.gitkeep
        # !data/timescale/.gitkeep

        # IDEs
        .idea/
        .vscode/
        *.suo
        *.ntvs*
        *.njsproj
        *.sln
        *.sw?
        ```
* **File(s) to Modify/Create**: `USTE/.gitignore`
* **Verification**:
    * Check that `USTE/.gitignore` exists and contains the specified content.

## Phase 2: Dockerized Databases Setup

**Task 2.1: Create `docker-compose.yml` for Databases**
* **Goal**: Define Docker Compose services for TimescaleDB and ArangoDB.
* **Action(s)**:
    1.  Create a file named `docker-compose.yml` in the `USTE/` directory.
    2.  Add the following content (as per the original README):
        ```yaml
        version: '3.8'
        services:
          timescaledb:
            image: timescale/timescaledb:latest-pg14 # Or a specific version like 2.10.0-pg14
            container_name: uste_timescaledb
            ports:
              - "5432:5432"
            volumes:
              - ./data/timescale:/var/lib/postgresql/data
            environment:
              POSTGRES_USER: postgres
              POSTGRES_PASSWORD: password
            restart: unless-stopped

          arangodb:
            image: arangodb/arangodb:latest # Or a specific version like 3.10.5
            container_name: uste_arangodb
            ports:
              - "8529:8529"
            volumes:
              - ./data/arango:/var/lib/arangodb3
            environment:
              ARANGO_ROOT_PASSWORD: password
              ARANGO_NO_AUTH: "false" # Ensure authentication is enabled
            restart: unless-stopped

        volumes:
          timescale_data:
            driver_opts:
              type: none
              device: ${PWD}/data/timescale
              o: bind
          arango_data:
            driver_opts:
              type: none
              device: ${PWD}/data/arango
              o: bind
        ```
* **File(s) to Modify/Create**: `USTE/docker-compose.yml`
* **Verification**:
    * `docker-compose.yml` exists and contains the correct service definitions.
    * Run `docker compose up -d` in `USTE/`. Both containers (`uste_timescaledb`, `uste_arangodb`) should start without errors.
    * Check `docker ps` to see them running.
    * TimescaleDB should be accessible on `localhost:5432`. ArangoDB UI should be accessible at `http://localhost:8529` (user: `root`, pass: `password`).
    * Run `docker compose down` to stop them for now.

## Phase 3: Basic FastAPI Application

**Task 3.1: Add FastAPI and Uvicorn to `requirements.txt`**
* **Goal**: Specify FastAPI and Uvicorn as project dependencies.
* **Action(s)**:
    1.  Add the following lines to `USTE/requirements.txt`:
        ```
        fastapi
        uvicorn[standard]
        ```
* **File(s) to Modify/Create**: `USTE/requirements.txt`
* **Verification**:
    * The file `USTE/requirements.txt` contains `fastapi` and `uvicorn[standard]`.
    * (Optional) Create a virtual environment and run `pip install -r requirements.txt`. It should install successfully.

**Task 3.2: Create `src/app/main.py` with a Basic FastAPI App Instance**
* **Goal**: Initialize the FastAPI application.
* **Action(s)**:
    1.  Create a file `USTE/src/app/main.py`.
    2.  Add the following content:
        ```python
        from fastapi import FastAPI

        app = FastAPI(title="USTE API", version="0.1.0")

        # Placeholder for future startup/shutdown events
        @app.on_event("startup")
        async def startup_event():
            print("USTE API starting up...")

        @app.on_event("shutdown")
        async def shutdown_event():
            print("USTE API shutting down...")
        ```
* **File(s) to Modify/Create**: `USTE/src/app/main.py`
* **Verification**:
    * The file `USTE/src/app/main.py` exists and contains the FastAPI app instantiation.
    * The code should be syntactically correct.

**Task 3.3: Add a Root Health Check Endpoint to `src/app/main.py`**
* **Goal**: Create a simple `/` endpoint to verify the app is running.
* **Action(s)**:
    1.  Modify `USTE/src/app/main.py` to add a root GET endpoint:
        ```python
        from fastapi import FastAPI

        app = FastAPI(title="USTE API", version="0.1.0")

        @app.on_event("startup")
        async def startup_event():
            print("USTE API starting up...")

        @app.on_event("shutdown")
        async def shutdown_event():
            print("USTE API shutting down...")

        @app.get("/", tags=["Health Check"])
        async def root():
            return {"message": "USTE API is running"}
        ```
* **File(s) to Modify/Create**: `USTE/src/app/main.py`
* **Verification**:
    * Run the FastAPI application from the `USTE/` directory: `uvicorn src.app.main:app --reload`.
    * Open a browser or use `curl` to access `http://localhost:8000/`.
    * The response should be `{"message":"USTE API is running"}`.
    * The console should show "USTE API starting up...". Stop the server (Ctrl+C), it should show "USTE API shutting down...".

## Phase 4: Database Connectivity Layer

**Task 4.1: Add Database Client Libraries to `requirements.txt`**
* **Goal**: Add Python libraries for connecting to TimescaleDB (psycopg2) and ArangoDB (python-arango).
* **Action(s)**:
    1.  Add the following lines to `USTE/requirements.txt`:
        ```
        psycopg2-binary # For PostgreSQL/TimescaleDB
        python-arango # For ArangoDB
        ```
* **File(s) to Modify/Create**: `USTE/requirements.txt`
* **Verification**:
    * `USTE/requirements.txt` is updated.
    * (Optional) In your virtual environment, run `pip install -r requirements.txt`. The new packages should install.

**Task 4.2: Create `src/database/db.py` with Placeholder Connection Logic**
* **Goal**: Initialize the database module and configuration placeholders.
* **Action(s)**:
    1.  Create `USTE/src/database/db.py`.
    2.  Add the following placeholder content:
        ```python
        # Database connection settings (replace with environment variables later)
        TIMESCALE_DATABASE_URL = "postgresql://postgres:password@localhost:5432/postgres" # Default, will need db creation
        ARANGO_DATABASE_URL = "http://localhost:8529"
        ARANGO_USERNAME = "root"
        ARANGO_PASSWORD = "password"
        ARANGO_DB_NAME = "uste_db" # Define a database name

        # Placeholder for TimescaleDB connection pool
        timescale_pool = None

        # Placeholder for ArangoDB client and database instance
        arango_client = None
        arango_db = None

        async def connect_to_databases():
            global timescale_pool, arango_client, arango_db
            print("Attempting to connect to databases...")
            # TODO: Implement TimescaleDB connection
            # TODO: Implement ArangoDB connection and database/collection creation
            print("Database connection placeholders executed.")

        async def close_database_connections():
            global timescale_pool, arango_client
            print("Attempting to close database connections...")
            # TODO: Implement TimescaleDB connection closing
            # TODO: Implement ArangoDB client closing (if applicable)
            print("Database connection closing placeholders executed.")

        # Helper function to get ArangoDB instance (example)
        def get_arango_db():
            if not arango_db:
                raise Exception("ArangoDB not initialized. Call connect_to_databases() first.")
            return arango_db
        ```
* **File(s) to Modify/Create**: `USTE/src/database/db.py`
* **Verification**:
    * `USTE/src/database/db.py` exists and contains the placeholder code.
    * Code is syntactically correct.

**Task 4.3: Integrate Database Connection Lifecycle into FastAPI `main.py`**
* **Goal**: Call connect/close database functions during FastAPI startup/shutdown.
* **Action(s)**:
    1.  Modify `USTE/src/app/main.py`:
        ```python
        from fastapi import FastAPI
        from src.database import db # Import the db module

        app = FastAPI(title="USTE API", version="0.1.0")

        @app.on_event("startup")
        async def startup_event():
            print("USTE API starting up...")
            await db.connect_to_databases() # Call connect

        @app.on_event("shutdown")
        async def shutdown_event():
            print("USTE API shutting down...")
            await db.close_database_connections() # Call close

        @app.get("/", tags=["Health Check"])
        async def root():
            return {"message": "USTE API is running"}
        ```
* **File(s) to Modify/Create**: `USTE/src/app/main.py`
* **Verification**:
    * Run `uvicorn src.app.main:app --reload`.
    * The console output should include:
        ```
        USTE API starting up...
        Attempting to connect to databases...
        Database connection placeholders executed.
        ```
    * When stopping the server (Ctrl+C), the console should include:
        ```
        USTE API shutting down...
        Attempting to close database connections...
        Database connection closing placeholders executed.
        ```

**Task 4.4: Implement ArangoDB Connection and Database Creation in `db.py`**
* **Goal**: Establish a connection to ArangoDB and ensure the target database exists.
* **Action(s)**:
    1. Start the Docker containers: `docker compose up -d`
    2. Modify `USTE/src/database/db.py`:
        ```python
        from arango import ArangoClient
        from arango.exceptions import DatabaseCreateError

        # Database connection settings
        TIMESCALE_DATABASE_URL = "postgresql://postgres:password@localhost:5432/postgres"
        ARANGO_DATABASE_URL = "http://localhost:8529"
        ARANGO_USERNAME = "root"
        ARANGO_PASSWORD = "password"
        ARANGO_DB_NAME = "uste_db"

        timescale_pool = None
        arango_client_instance = None # Renamed to avoid conflict with arango.ArangoClient
        arango_db_instance = None # Renamed

        async def connect_to_databases():
            global timescale_pool, arango_client_instance, arango_db_instance
            print("Attempting to connect to ArangoDB...")
            try:
                # Initialize ArangoDB client
                client = ArangoClient(hosts=ARANGO_DATABASE_URL)
                arango_client_instance = client

                # Connect to ArangoDB system database to create the target database
                sys_db = client.db("_system", username=ARANGO_USERNAME, password=ARANGO_PASSWORD)

                # Create the target database if it doesn't exist
                if not sys_db.has_database(ARANGO_DB_NAME):
                    sys_db.create_database(ARANGO_DB_NAME)
                    print(f"ArangoDB database '{ARANGO_DB_NAME}' created.")
                else:
                    print(f"ArangoDB database '{ARANGO_DB_NAME}' already exists.")

                # Connect to the target database
                arango_db_instance = client.db(ARANGO_DB_NAME, username=ARANGO_USERNAME, password=ARANGO_PASSWORD)
                print(f"Successfully connected to ArangoDB database '{ARANGO_DB_NAME}'.")

            except Exception as e:
                print(f"Error connecting to ArangoDB: {e}")
                # Potentially raise an exception or handle appropriately

            # TODO: Implement TimescaleDB connection
            print("TimescaleDB connection placeholder.")


        async def close_database_connections():
            global timescale_pool, arango_client_instance
            print("Attempting to close ArangoDB connection...")
            # ArangoDB client does not have an explicit close method for HTTP connections.
            # Connections are typically managed per request or pooled by the client.
            # We can clear our reference.
            arango_client_instance = None
            arango_db_instance = None
            print("ArangoDB client reference cleared.")

            # TODO: Implement TimescaleDB connection closing
            print("TimescaleDB closing placeholder.")


        def get_arango_db():
            if not arango_db_instance:
                raise Exception("ArangoDB not initialized. Call connect_to_databases() during app startup.")
            return arango_db_instance
        ```
* **File(s) to Modify/Create**: `USTE/src/database/db.py`
* **Verification**:
    * Ensure ArangoDB is running via `docker compose up -d`.
    * Run `uvicorn src.app.main:app --reload`.
    * Console output should show successful connection messages for ArangoDB, including database creation if it's the first run.
    * Check ArangoDB UI (`http://localhost:8529`) to confirm the `uste_db` database exists.

**Task 4.5: Implement TimescaleDB Connection in `db.py`**
* **Goal**: Establish a connection pool to TimescaleDB.
* **Action(s)**:
    1.  Ensure TimescaleDB Docker container is running: `docker compose up -d`.
    2.  Modify `USTE/src/database/db.py`. Add imports and update `connect_to_databases` and `close_database_connections`:
        ```python
        import asyncpg # For TimescaleDB (PostgreSQL)
        from arango import ArangoClient
        from arango.exceptions import DatabaseCreateError

        # ... (ARANGO settings remain the same) ...
        TIMESCALE_DATABASE_URL = "postgresql://postgres:password@localhost:5432/postgres" # Ensure this DB exists
        # For a dedicated DB: "postgresql://postgres:password@localhost:5432/uste_timescale_db"
        # If using a dedicated DB, it needs to be created first. For MVP, 'postgres' is fine.

        timescale_pool = None
        arango_client_instance = None
        arango_db_instance = None

        async def connect_to_databases():
            global timescale_pool, arango_client_instance, arango_db_instance
            # ArangoDB connection (from previous task)
            print("Attempting to connect to ArangoDB...")
            try:
                client = ArangoClient(hosts=ARANGO_DATABASE_URL)
                arango_client_instance = client
                sys_db = client.db("_system", username=ARANGO_USERNAME, password=ARANGO_PASSWORD)
                if not sys_db.has_database(ARANGO_DB_NAME):
                    sys_db.create_database(ARANGO_DB_NAME)
                    print(f"ArangoDB database '{ARANGO_DB_NAME}' created.")
                else:
                    print(f"ArangoDB database '{ARANGO_DB_NAME}' already exists.")
                arango_db_instance = client.db(ARANGO_DB_NAME, username=ARANGO_USERNAME, password=ARANGO_PASSWORD)
                print(f"Successfully connected to ArangoDB database '{ARANGO_DB_NAME}'.")
            except Exception as e:
                print(f"Error connecting to ArangoDB: {e}")


            print("Attempting to connect to TimescaleDB...")
            try:
                timescale_pool = await asyncpg.create_pool(TIMESCALE_DATABASE_URL, min_size=1, max_size=10)
                print("Successfully connected to TimescaleDB and connection pool created.")
                # You might want to create the 'uste_timescale_db' database and enable timescaledb extension
                # For MVP, connecting to default 'postgres' database is simpler.
                # async with timescale_pool.acquire() as connection:
                #     await connection.execute("CREATE EXTENSION IF NOT EXISTS timescaledb;")
                #     print("TimescaleDB extension enabled (if not already).")
            except Exception as e:
                print(f"Error connecting to TimescaleDB: {e}")


        async def close_database_connections():
            global timescale_pool, arango_client_instance, arango_db_instance
            # ArangoDB (from previous task)
            print("Attempting to close ArangoDB connection...")
            arango_client_instance = None
            arango_db_instance = None
            print("ArangoDB client reference cleared.")

            print("Attempting to close TimescaleDB connections...")
            if timescale_pool:
                await timescale_pool.close()
                print("TimescaleDB connection pool closed.")
            timescale_pool = None


        def get_arango_db():
            # ... (same as before)
            if not arango_db_instance:
                raise Exception("ArangoDB not initialized. Call connect_to_databases() during app startup.")
            return arango_db_instance

        async def get_timescale_conn():
            if not timescale_pool:
                raise Exception("TimescaleDB pool not initialized. Call connect_to_databases() during app startup.")
            return await timescale_pool.acquire()

        async def release_timescale_conn(conn):
            if timescale_pool and conn:
                await timescale_pool.release(conn)
        ```
* **File(s) to Modify/Create**: `USTE/src/database/db.py`
* **Verification**:
    * Ensure TimescaleDB Docker container is running.
    * Run `uvicorn src.app.main:app --reload`.
    * Console output should show successful connection messages for both ArangoDB and TimescaleDB.
    * No errors related to database connections should appear.

## Phase 5: Basic Data Models (Pydantic)

**Task 5.1: Create `src/app/models.py`**
* **Goal**: Create the file for Pydantic models.
* **Action(s)**:
    1.  Create an empty file `USTE/src/app/models.py`.
* **File(s) to Modify/Create**: `USTE/src/app/models.py`
* **Verification**:
    * The file `USTE/src/app/models.py` exists.

**Task 5.2: Define a Basic Pydantic Model for a `Node` in `models.py`**
* **Goal**: Define the schema for a generic node.
* **Action(s)**:
    1.  Add the following to `USTE/src/app/models.py`:
        ```python
        from pydantic import BaseModel, Field
        from typing import Optional, Dict, Any

        class NodeBase(BaseModel):
            label: str = Field(..., description="Label or type of the node")
            properties: Optional[Dict[str, Any]] = Field(default_factory=dict, description="Arbitrary properties for the node")

        class NodeCreate(NodeBase):
            pass

        class Node(NodeBase):
            id: str = Field(..., description="Unique identifier of the node (e.g., ArangoDB _key or _id)")

            class Config:
                orm_mode = True # For compatibility if using ORM-like responses
                # Starting from Pydantic V2, `orm_mode` is deprecated. Use `from_attributes = True`
                # from_attributes = True # For Pydantic V2
        ```
    * **Note**: If using Pydantic V2, change `orm_mode = True` to `from_attributes = True`. Add `pydantic>=2.0` or `<2.0` to `requirements.txt` if specific version is needed. For now, assume Pydantic V1.x syntax for `orm_mode`.
* **File(s) to Modify/Create**: `USTE/src/app/models.py`
* **Verification**:
    * `USTE/src/app/models.py` contains the `NodeBase`, `NodeCreate`, and `Node` Pydantic models.
    * The code is syntactically correct.

**Task 5.3: Define a Basic Pydantic Model for an `Edge` in `models.py`**
* **Goal**: Define the schema for a generic edge.
* **Action(s)**:
    1.  Add the following to `USTE/src/app/models.py`:
        ```python
        # ... (Node models from previous task) ...

        class EdgeBase(BaseModel):
            label: Optional[str] = Field(None, description="Label or type of the edge")
            properties: Optional[Dict[str, Any]] = Field(default_factory=dict, description="Arbitrary properties for the edge")

        class EdgeCreate(EdgeBase):
            from_node_id: str = Field(..., description="ID of the source node (ArangoDB _key)")
            to_node_id: str = Field(..., description="ID of the target node (ArangoDB _key)")

        class Edge(EdgeBase):
            id: str = Field(..., description="Unique identifier of the edge (e.g., ArangoDB _key or _id)")
            from_node_id: str = Field(..., description="Full ID of the source node (ArangoDB _id)")
            to_node_id: str = Field(..., description="Full ID of the target node (ArangoDB _id)")

            class Config:
                orm_mode = True
                # from_attributes = True # For Pydantic V2
        ```
* **File(s) to Modify/Create**: `USTE/src/app/models.py`
* **Verification**:
    * `USTE/src/app/models.py` now also contains the `EdgeBase`, `EdgeCreate`, and `Edge` Pydantic models.
    * The code is syntactically correct.

## Phase 6: API Endpoint - Create Node (ArangoDB)

**Task 6.1: Create Node Collection in ArangoDB on Startup**
* **Goal**: Ensure a default 'nodes' collection exists in ArangoDB.
* **Action(s)**:
    1.  Modify `USTE/src/database/db.py` within the `connect_to_databases` function, after connecting to `arango_db_instance`:
        ```python
        # ... inside connect_to_databases, after arango_db_instance is set ...
        if arango_db_instance:
            if not arango_db_instance.has_collection("nodes"):
                arango_db_instance.create_collection("nodes")
                print("ArangoDB 'nodes' collection created.")
            else:
                print("ArangoDB 'nodes' collection already exists.")

            if not arango_db_instance.has_collection("edges"): # Also create edges collection
                # For edges, it must be an edge collection
                arango_db_instance.create_collection("edges", edge=True)
                print("ArangoDB 'edges' collection created.")
            else:
                print("ArangoDB 'edges' collection already exists.")
        # ...
        ```
* **File(s) to Modify/Create**: `USTE/src/database/db.py`
* **Verification**:
    * Run `uvicorn src.app.main:app --reload`.
    * Console should indicate that "nodes" and "edges" collections are created or already exist.
    * Verify in ArangoDB UI that these collections exist in `uste_db`.

**Task 6.2: Add `POST /nodes` Endpoint Definition in `main.py`**
* **Goal**: Define the API endpoint for creating nodes.
* **Action(s)**:
    1.  Modify `USTE/src/app/main.py` to add the new endpoint:
        ```python
        from fastapi import FastAPI, HTTPException, status
        from src.database import db
        from src.app import models # Import your Pydantic models

        # ... (app instance and startup/shutdown events) ...

        @app.post("/nodes", response_model=models.Node, status_code=status.HTTP_201_CREATED, tags=["Nodes"])
        async def create_node(node_in: models.NodeCreate):
            # Placeholder: Actual database logic will be added next
            print(f"Received node creation request: {node_in.model_dump_json()}") # Use model_dump_json for Pydantic V2
            # For Pydantic V1: node_in.json()
            # This is a placeholder response
            return models.Node(id="temp_id_123", label=node_in.label, properties=node_in.properties)
        ```
        * **Note**: If using Pydantic V2, replace `node_in.json()` with `node_in.model_dump_json()`. And `Node(**node_in.model_dump(), id="temp_id_123")` might be safer for constructing the response.
* **File(s) to Modify/Create**: `USTE/src/app/main.py`
* **Verification**:
    * Run `uvicorn src.app.main:app --reload`.
    * Access API docs at `http://localhost:8000/docs`. The `/nodes` POST endpoint should be visible.
    * Send a POST request (e.g., via curl or API docs UI) to `/nodes` with JSON body like `{"label": "Person", "properties": {"name": "Alice"}}`.
    * The server should respond with HTTP 201 and a JSON body like `{"label":"Person","properties":{"name":"Alice"},"id":"temp_id_123"}`.
    * Console should print the received node data.

**Task 6.3: Implement Logic to Save Node to ArangoDB in `create_node` Endpoint**
* **Goal**: Persist the new node data into the ArangoDB 'nodes' collection.
* **Action(s)**:
    1.  Modify the `create_node` function in `USTE/src/app/main.py`:
        ```python
        # ... (imports and other code) ...
        @app.post("/nodes", response_model=models.Node, status_code=status.HTTP_201_CREATED, tags=["Nodes"])
        async def create_node(node_in: models.NodeCreate):
            arango_db_conn = db.get_arango_db()
            nodes_collection = arango_db_conn.collection("nodes")
            try:
                # Prepare document for ArangoDB
                node_doc = {"label": node_in.label, "properties": node_in.properties}
                meta = nodes_collection.insert(node_doc) # meta contains _id, _key, _rev
                
                # Construct the response using data from ArangoDB
                # ArangoDB _id includes collection name, _key is unique within collection
                created_node_data = {
                    "id": meta["_key"], # Use _key as the simple ID for response
                    "label": node_in.label,
                    "properties": node_in.properties
                }
                return models.Node(**created_node_data)
            except Exception as e:
                # Log the exception e
                print(f"Error inserting node: {e}")
                raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Could not create node")
        ```
* **File(s) to Modify/Create**: `USTE/src/app/main.py`
* **Verification**:
    * Run `uvicorn src.app.main:app --reload`.
    * Send a POST request to `/nodes` with JSON body (e.g., `{"label": "City", "properties": {"name": "New York"}}`).
    * Server should respond with HTTP 201 and the created node data, including an `id` assigned by ArangoDB (the `_key`).
    * Verify in ArangoDB UI (`aste_db` -> `nodes` collection) that the new document exists with the correct data.

## Phase 7: API Endpoint - Get Node (ArangoDB)

**Task 7.1: Add `GET /nodes/{node_id}` Endpoint Definition in `main.py`**
* **Goal**: Define an API endpoint to retrieve a specific node by its ID.
* **Action(s)**:
    1.  Modify `USTE/src/app/main.py` to add the new endpoint:
        ```python
        # ... (imports and other code) ...
        @app.get("/nodes/{node_id}", response_model=models.Node, tags=["Nodes"])
        async def get_node(node_id: str):
            arango_db_conn = db.get_arango_db()
            nodes_collection = arango_db_conn.collection("nodes")
            try:
                node_doc = nodes_collection.get(node_id) # Uses _key to fetch
                if not node_doc:
                    raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Node not found")
                
                # Construct response
                retrieved_node_data = {
                    "id": node_doc["_key"],
                    "label": node_doc.get("label"), # Use .get for safety if fields are optional
                    "properties": node_doc.get("properties", {})
                }
                return models.Node(**retrieved_node_data)
            except Exception as e:
                # Log the exception e if it's not an HTTPException we raised
                if not isinstance(e, HTTPException):
                    print(f"Error retrieving node: {e}")
                    raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Could not retrieve node")
                raise # Re-raise HTTPException
        ```
* **File(s) to Modify/Create**: `USTE/src/app/main.py`
* **Verification**:
    * Run `uvicorn src.app.main:app --reload`.
    * First, create a node using the `POST /nodes` endpoint and note its `id` (e.g., "12345").
    * Send a GET request to `/nodes/12345` (replace with actual ID).
    * Server should respond with HTTP 200 and the node's data.
    * Send a GET request to `/nodes/nonexistentid`.
    * Server should respond with HTTP 404.

## Phase 8: API Endpoint - Create Edge (ArangoDB)

**Task 8.1: Add `POST /edges` Endpoint Definition in `main.py`**
* **Goal**: Define an API endpoint for creating edges between nodes.
* **Action(s)**:
    1.  Modify `USTE/src/app/main.py`:
        ```python
        # ... (imports and other code) ...
        @app.post("/edges", response_model=models.Edge, status_code=status.HTTP_201_CREATED, tags=["Edges"])
        async def create_edge(edge_in: models.EdgeCreate):
            arango_db_conn = db.get_arango_db()
            edges_collection = arango_db_conn.collection("edges")
            nodes_collection = arango_db_conn.collection("nodes") # To verify nodes exist

            # ArangoDB expects _id format for _from and _to, like "nodes/node_key"
            from_node_full_id = f"nodes/{edge_in.from_node_id}"
            to_node_full_id = f"nodes/{edge_in.to_node_id}"

            # Optional: Check if 'from' and 'to' nodes exist
            if not nodes_collection.has(edge_in.from_node_id) or not nodes_collection.has(edge_in.to_node_id):
                 raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="Source or target node not found")

            try:
                edge_doc = {
                    "_from": from_node_full_id,
                    "_to": to_node_full_id,
                    "label": edge_in.label,
                    "properties": edge_in.properties
                }
                meta = edges_collection.insert(edge_doc)
                
                created_edge_data = {
                    "id": meta["_key"],
                    "label": edge_in.label,
                    "properties": edge_in.properties,
                    "from_node_id": from_node_full_id, # Return full _id as per model
                    "to_node_id": to_node_full_id     # Return full _id as per model
                }
                return models.Edge(**created_edge_data)
            except Exception as e:
                print(f"Error inserting edge: {e}")
                raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Could not create edge")
        ```
* **File(s) to Modify/Create**: `USTE/src/app/main.py`
* **Verification**:
    * Run `uvicorn src.app.main:app --reload`.
    * Create two nodes (e.g., nodeA with id `keyA`, nodeB with id `keyB`) using `POST /nodes`.
    * Send a POST request to `/edges` with body like `{"from_node_id": "keyA", "to_node_id": "keyB", "label": "connected_to"}`.
    * Server should respond with HTTP 201 and the created edge data.
    * Verify in ArangoDB UI (`uste_db` -> `edges` collection) that the new edge exists and correctly links `nodes/keyA` to `nodes/keyB`.

## Phase 9: Basic TimescaleDB Table & API (Placeholder)

**Task 9.1: Define Pydantic Model for a `TimeSeriesData` in `models.py`**
* **Goal**: Define schema for a simple time-series entry.
* **Action(s)**:
    1.  Add to `USTE/src/app/models.py`:
        ```python
        import datetime # Add this import at the top

        # ... (Node and Edge models) ...

        class TimeSeriesDataBase(BaseModel):
            metric_name: str
            value: float
            tags: Optional[Dict[str, str]] = None

        class TimeSeriesDataCreate(TimeSeriesDataBase):
            timestamp: Optional[datetime.datetime] = Field(default_factory=datetime.datetime.utcnow)

        class TimeSeriesData(TimeSeriesDataBase):
            id: int # Assuming an auto-incrementing ID from SQL
            timestamp: datetime.datetime

            class Config:
                orm_mode = True
                # from_attributes = True # For Pydantic V2
        ```
* **File(s) to Modify/Create**: `USTE/src/app/models.py`
* **Verification**:
    * `USTE/src/app/models.py` contains the new TimeSeriesData models. Code is syntactically correct.

**Task 9.2: Create Basic TimescaleDB Table on Startup (e.g., `metrics`)**
* **Goal**: Create a simple hypertable in TimescaleDB for storing metrics.
* **Action(s)**:
    1.  Modify `USTE/src/database/db.py` in `connect_to_databases`:
        ```python
        # ... (inside connect_to_databases, after TimescaleDB pool is created) ...
        if timescale_pool:
            async with timescale_pool.acquire() as conn:
                async with conn.transaction():
                    # Enable TimescaleDB extension if not already enabled (idempotent)
                    await conn.execute("CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;")
                    
                    # Create a regular SQL table
                    await conn.execute("""
                        CREATE TABLE IF NOT EXISTS metrics (
                            id SERIAL PRIMARY KEY,
                            time TIMESTAMPTZ NOT NULL,
                            metric_name TEXT NOT NULL,
                            value DOUBLE PRECISION NOT NULL,
                            tags JSONB
                        );
                    """)
                    # Convert to hypertable, IF NOT EXISTS is tricky here, so check first
                    is_hypertable = await conn.fetchval("""
                        SELECT EXISTS (
                            SELECT 1 FROM timescaledb_information.hypertables
                            WHERE hypertable_name = 'metrics'
                        );
                    """)
                    if not is_hypertable:
                        await conn.execute("SELECT create_hypertable('metrics', 'time', if_not_exists => TRUE);")
                        print("TimescaleDB 'metrics' table created/verified as hypertable.")
                    else:
                        print("TimescaleDB 'metrics' hypertable already exists.")
        # ...
        ```
* **File(s) to Modify/Create**: `USTE/src/database/db.py`
* **Verification**:
    * Run `uvicorn src.app.main:app --reload`.
    * Console should indicate successful creation/verification of the `metrics` hypertable.
    * Connect to TimescaleDB (e.g., using `psql` or a DB tool) and verify the `metrics` table structure and that it's a hypertable (`\d metrics`, check TimescaleDB specific commands).

**Task 9.3: Add `POST /metrics` Endpoint Definition in `main.py`**
* **Goal**: Define an API endpoint for ingesting time-series data.
* **Action(s)**:
    1.  Modify `USTE/src/app/main.py`:
        ```python
        # ... (imports and other code) ...
        import json # for tags

        @app.post("/metrics", response_model=models.TimeSeriesData, status_code=status.HTTP_201_CREATED, tags=["Metrics"])
        async def create_metric(metric_in: models.TimeSeriesDataCreate):
            conn = None
            try:
                conn = await db.get_timescale_conn()
                # Convert dict tags to JSON string for PostgreSQL JSONB
                tags_json = json.dumps(metric_in.tags) if metric_in.tags else None

                row = await conn.fetchrow(
                    """
                    INSERT INTO metrics (time, metric_name, value, tags)
                    VALUES ($1, $2, $3, $4)
                    RETURNING id, time, metric_name, value, tags;
                    """,
                    metric_in.timestamp, metric_in.metric_name, metric_in.value, tags_json
                )
                # Parse tags back from JSON string to dict for response model
                db_tags = json.loads(row['tags']) if row['tags'] else None

                return models.TimeSeriesData(
                    id=row['id'],
                    timestamp=row['time'],
                    metric_name=row['metric_name'],
                    value=row['value'],
                    tags=db_tags
                )
            except Exception as e:
                print(f"Error inserting metric: {e}")
                raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Could not create metric")
            finally:
                if conn:
                    await db.release_timescale_conn(conn)
        ```
* **File(s) to Modify/Create**: `USTE/src/app/main.py`
* **Verification**:
    * Run `uvicorn src.app.main:app --reload`.
    * Send a POST request to `/metrics` with body like `{"metric_name": "temperature", "value": 25.5, "tags": {"sensor_id": "A1"}}`.
    * Server should respond with HTTP 201 and the created metric data, including `id` and `timestamp`.
    * Verify in TimescaleDB that the new row exists in the `metrics` table.

## Phase 10: Basic Unit Tests

**Task 10.1: Add `pytest` and `httpx` to `requirements.txt`**
* **Goal**: Add libraries for testing.
* **Action(s)**:
    1.  Add to `USTE/requirements.txt`:
        ```
        pytest
        httpx # For making HTTP requests in tests
        # pytest-asyncio (if not already pulled by fastapi testing tools)
        ```
* **File(s) to Modify/Create**: `USTE/requirements.txt`
* **Verification**: `requirements.txt` is updated. `pip install -r requirements.txt` installs them.

**Task 10.2: Create `tests/test_app.py` with a Test for Root Endpoint**
* **Goal**: Set up the test file and a basic test.
* **Action(s)**:
    1.  Create `USTE/tests/test_app.py`:
        ```python
        import pytest
        from httpx import AsyncClient
        from src.app.main import app # Import your FastAPI app

        @pytest.mark.asyncio
        async def test_root_health_check():
            async with AsyncClient(app=app, base_url="http://test") as ac:
                response = await ac.get("/")
            assert response.status_code == 200
            assert response.json() == {"message": "USTE API is running"}
        ```
* **File(s) to Modify/Create**: `USTE/tests/test_app.py`
* **Verification**:
    * Run `pytest` from the `USTE/` directory.
    * The `test_root_health_check` should pass.

**Task 10.3: Add a Test for `POST /nodes` and `GET /nodes/{node_id}`**
* **Goal**: Test basic node creation and retrieval.
* **Action(s)**:
    1.  Add to `USTE/tests/test_app.py`:
        ```python
        # ... (imports from previous task) ...

        @pytest.mark.asyncio
        async def test_create_and_get_node():
            async with AsyncClient(app=app, base_url="http://test") as ac:
                # Ensure ArangoDB is clean or use unique data for test
                # For a real test suite, you'd clear the DB or use specific test collections.
                # For this MVP step, we assume it can run against the dev DB.
                
                node_data = {"label": "TestNode", "properties": {"name": "My Test Node", "value": 123}}
                response_create = await ac.post("/nodes", json=node_data)
                assert response_create.status_code == 201
                created_node = response_create.json()
                assert created_node["label"] == node_data["label"]
                assert created_node["properties"] == node_data["properties"]
                assert "id" in created_node

                node_id = created_node["id"]
                response_get = await ac.get(f"/nodes/{node_id}")
                assert response_get.status_code == 200
                retrieved_node = response_get.json()
                assert retrieved_node["id"] == node_id
                assert retrieved_node["label"] == node_data["label"]
                assert retrieved_node["properties"] == node_data["properties"]

                # Test getting a non-existent node
                response_get_non_existent = await ac.get("/nodes/nonexistentnodeid123")
                assert response_get_non_existent.status_code == 404
        ```
* **File(s) to Modify/Create**: `USTE/tests/test_app.py`
* **Verification**:
    * Ensure Docker databases are running (`docker compose up -d`).
    * Run `pytest` from the `USTE/` directory.
    * All tests, including `test_create_and_get_node`, should pass. (Note: Test assumes ArangoDB is running and accessible as configured for the app).

This concludes the initial MVP build plan. Further steps would involve more complex queries, other API endpoints, embedding integration, GPU tasks, more robust error handling, configuration management, and comprehensive testing.