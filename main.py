import uvicorn
from src.deepseek_web_api.core.config import (
    get_server_host,
    get_server_port,
    get_server_reload,
)
from src.deepseek_web_api.core.server_security import validate_startup_config

if __name__ == "__main__":
    validate_startup_config()
    uvicorn.run(
        "src.deepseek_web_api:app",
        host=get_server_host(),
        port=get_server_port(),
        reload=get_server_reload(),
    )
