"""DeepSeek Web API routes."""

import logging

from fastapi import FastAPI, Request, Response
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import StreamingResponse

from .chat_completions_service import (
    stream_chat_completion,
    create_session_on_deepseek,
    delete_session as delete_session_service,
    create_session,
    upload_file,
    fetch_files,
    get_history_messages,
)

# API path constants - kept for reference, actual paths defined in chat_completions_service

logger = logging.getLogger("deepseek_web_api")

app = FastAPI()

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.api_route("/v0/chat/completion", methods=["POST"])
async def completion(request: Request):
    """Send chat completion with streaming SSE response."""
    body = await request.json()
    prompt = body.pop("prompt")
    search_enabled = body.pop("search_enabled", True)
    thinking_enabled = body.pop("thinking_enabled", True)
    client_chat_session_id = body.pop("chat_session_id", None)
    ref_file_ids = body.get("ref_file_ids", [])

    # Pre-create session if needed to return session_id in header
    response_headers = {}
    if client_chat_session_id is None:
        chat_session_id = await create_session_on_deepseek()
        client_chat_session_id = chat_session_id
        response_headers["X-Chat-Session-Id"] = chat_session_id

    async def stream_and_set_header():
        async for chunk in stream_chat_completion(
            prompt=prompt,
            chat_session_id=client_chat_session_id,
            search_enabled=search_enabled,
            thinking_enabled=thinking_enabled,
            ref_file_ids=ref_file_ids,
        ):
            yield chunk

    return StreamingResponse(
        stream_and_set_header(),
        media_type="text/event-stream",
        headers=response_headers if response_headers else None,
    )


@app.api_route("/v0/chat/delete", methods=["POST"])
async def delete_session(request: Request):
    """Delete session."""
    body = await request.json()
    chat_session_id = body.get("chat_session_id")

    return await delete_session_service(chat_session_id)


@app.api_route("/v0/chat/create_session", methods=["POST"])
async def create_session_route(request: Request):
    """Create new session."""
    body = await request.json()
    return await create_session(body)


@app.api_route("/v0/chat/upload_file", methods=["POST"])
async def upload_file_route(request: Request):
    """Upload file."""
    form = await request.form()
    file = form.get("file")
    if not file:
        return Response(content="No file provided", status_code=400)

    file_content = await file.read()
    return await upload_file(file_content, file.filename, file.content_type)


@app.api_route("/v0/chat/fetch_files", methods=["GET"])
async def fetch_files_route(request: Request):
    """Fetch file status."""
    file_ids = request.query_params.get("file_ids")
    return await fetch_files(file_ids)


@app.api_route("/v0/chat/history_messages", methods=["GET"])
async def history_messages_route(request: Request):
    """Get chat history."""
    chat_session_id = request.query_params.get("chat_session_id")
    offset = request.query_params.get("offset", "0")
    limit = request.query_params.get("limit", "20")
    return await get_history_messages(chat_session_id, offset, limit)


@app.get("/")
async def index():
    return {"status": "ok", "service": "deepseek-web-api"}
