## List input items

**get** `/responses/{response_id}/input_items`

Returns a list of input items for a given response.

### Path Parameters

- `response_id: string`

### Query Parameters

- `after: optional string`

  An item ID to list items after, used in pagination.

- `include: optional array of ResponseIncludable`

  Additional fields to include in the response. See the `include`
  parameter for Response creation above for more information.

  - `"file_search_call.results"`

  - `"web_search_call.results"`

  - `"web_search_call.action.sources"`

  - `"message.input_image.image_url"`

  - `"computer_call_output.output.image_url"`

  - `"code_interpreter_call.outputs"`

  - `"reasoning.encrypted_content"`

  - `"message.output_text.logprobs"`

- `limit: optional number`

  A limit on the number of objects to be returned. Limit can range between
  1 and 100, and the default is 20.

- `order: optional "asc" or "desc"`

  The order to return the input items in. Default is `desc`.

  - `asc`: Return the input items in ascending order.
  - `desc`: Return the input items in descending order.

  - `"asc"`

  - `"desc"`

### Returns

- `ResponseItemList object { data, first_id, has_more, 2 more }`

  A list of Response items.

  - `data: array of ResponseInputMessageItem or ResponseOutputMessage or object { id, queries, status, 2 more }  or 23 more`

    A list of items used to generate this response.

    - `ResponseInputMessageItem object { id, content, role, 2 more }`

      - `id: string`

        The unique ID of the message input.

      - `content: ResponseInputMessageContentList`

        A list of one or many input items to the model, containing different content
        types.

        - `ResponseInputText object { text, type }`

          A text input to the model.

          - `text: string`

            The text input to the model.

          - `type: "input_text"`

            The type of the input item. Always `input_text`.

            - `"input_text"`

        - `ResponseInputImage object { detail, type, file_id, image_url }`

          An image input to the model. Learn about [image inputs](/docs/guides/vision).

          - `detail: "low" or "high" or "auto" or "original"`

            The detail level of the image to be sent to the model. One of `high`, `low`, `auto`, or `original`. Defaults to `auto`.

            - `"low"`

            - `"high"`

            - `"auto"`

            - `"original"`

          - `type: "input_image"`

            The type of the input item. Always `input_image`.

            - `"input_image"`

          - `file_id: optional string`

            The ID of the file to be sent to the model.

          - `image_url: optional string`

            The URL of the image to be sent to the model. A fully qualified URL or base64 encoded image in a data URL.

        - `ResponseInputFile object { type, detail, file_data, 3 more }`

          A file input to the model.

          - `type: "input_file"`

            The type of the input item. Always `input_file`.

            - `"input_file"`

          - `detail: optional "low" or "high"`

            The detail level of the file to be sent to the model. Use `low` for the default rendering behavior, or `high` to render the file at higher quality. Defaults to `low`.

            - `"low"`

            - `"high"`

          - `file_data: optional string`

            The content of the file to be sent to the model.

          - `file_id: optional string`

            The ID of the file to be sent to the model.

          - `file_url: optional string`

            The URL of the file to be sent to the model.

          - `filename: optional string`

            The name of the file to be sent to the model.

      - `role: "user" or "system" or "developer"`

        The role of the message input. One of `user`, `system`, or `developer`.

        - `"user"`

        - `"system"`

        - `"developer"`

      - `type: "message"`

        The type of the message input. Always set to `message`.

        - `"message"`

      - `status: optional "in_progress" or "completed" or "incomplete"`

        The status of item. One of `in_progress`, `completed`, or
        `incomplete`. Populated when items are returned via API.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

    - `ResponseOutputMessage object { id, content, role, 3 more }`

      An output message from the model.

      - `id: string`

        The unique ID of the output message.

      - `content: array of ResponseOutputText or ResponseOutputRefusal`

        The content of the output message.

        - `ResponseOutputText object { annotations, logprobs, text, type }`

          A text output from the model.

          - `annotations: array of object { file_id, filename, index, type }  or object { end_index, start_index, title, 2 more }  or object { container_id, end_index, file_id, 3 more }  or object { file_id, index, type }`

            The annotations of the text output.

            - `FileCitation object { file_id, filename, index, type }`

              A citation to a file.

              - `file_id: string`

                The ID of the file.

              - `filename: string`

                The filename of the file cited.

              - `index: number`

                The index of the file in the list of files.

              - `type: "file_citation"`

                The type of the file citation. Always `file_citation`.

                - `"file_citation"`

            - `URLCitation object { end_index, start_index, title, 2 more }`

              A citation for a web resource used to generate a model response.

              - `end_index: number`

                The index of the last character of the URL citation in the message.

              - `start_index: number`

                The index of the first character of the URL citation in the message.

              - `title: string`

                The title of the web resource.

              - `type: "url_citation"`

                The type of the URL citation. Always `url_citation`.

                - `"url_citation"`

              - `url: string`

                The URL of the web resource.

            - `ContainerFileCitation object { container_id, end_index, file_id, 3 more }`

              A citation for a container file used to generate a model response.

              - `container_id: string`

                The ID of the container file.

              - `end_index: number`

                The index of the last character of the container file citation in the message.

              - `file_id: string`

                The ID of the file.

              - `filename: string`

                The filename of the container file cited.

              - `start_index: number`

                The index of the first character of the container file citation in the message.

              - `type: "container_file_citation"`

                The type of the container file citation. Always `container_file_citation`.

                - `"container_file_citation"`

            - `FilePath object { file_id, index, type }`

              A path to a file.

              - `file_id: string`

                The ID of the file.

              - `index: number`

                The index of the file in the list of files.

              - `type: "file_path"`

                The type of the file path. Always `file_path`.

                - `"file_path"`

          - `logprobs: array of object { token, bytes, logprob, top_logprobs }`

            - `token: string`

            - `bytes: array of number`

            - `logprob: number`

            - `top_logprobs: array of object { token, bytes, logprob }`

              - `token: string`

              - `bytes: array of number`

              - `logprob: number`

          - `text: string`

            The text output from the model.

          - `type: "output_text"`

            The type of the output text. Always `output_text`.

            - `"output_text"`

        - `ResponseOutputRefusal object { refusal, type }`

          A refusal from the model.

          - `refusal: string`

            The refusal explanation from the model.

          - `type: "refusal"`

            The type of the refusal. Always `refusal`.

            - `"refusal"`

      - `role: "assistant"`

        The role of the output message. Always `assistant`.

        - `"assistant"`

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the message input. One of `in_progress`, `completed`, or
        `incomplete`. Populated when input items are returned via API.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "message"`

        The type of the output message. Always `message`.

        - `"message"`

      - `phase: optional "commentary" or "final_answer"`

        Labels an `assistant` message as intermediate commentary (`commentary`) or the final answer (`final_answer`).
        For models like `gpt-5.3-codex` and beyond, when sending follow-up requests, preserve and resend
        phase on all assistant messages — dropping it can degrade performance. Not used for user messages.

        - `"commentary"`

        - `"final_answer"`

    - `FileSearchCall object { id, queries, status, 2 more }`

      The results of a file search tool call. See the
      [file search guide](/docs/guides/tools-file-search) for more information.

      - `id: string`

        The unique ID of the file search tool call.

      - `queries: array of string`

        The queries used to search for files.

      - `status: "in_progress" or "searching" or "completed" or 2 more`

        The status of the file search tool call. One of `in_progress`,
        `searching`, `incomplete` or `failed`,

        - `"in_progress"`

        - `"searching"`

        - `"completed"`

        - `"incomplete"`

        - `"failed"`

      - `type: "file_search_call"`

        The type of the file search tool call. Always `file_search_call`.

        - `"file_search_call"`

      - `results: optional array of object { attributes, file_id, filename, 2 more }`

        The results of the file search tool call.

        - `attributes: optional map[string or number or boolean]`

          Set of 16 key-value pairs that can be attached to an object. This can be
          useful for storing additional information about the object in a structured
          format, and querying for objects via API or the dashboard. Keys are strings
          with a maximum length of 64 characters. Values are strings with a maximum
          length of 512 characters, booleans, or numbers.

          - `string`

          - `number`

          - `boolean`

        - `file_id: optional string`

          The unique ID of the file.

        - `filename: optional string`

          The name of the file.

        - `score: optional number`

          The relevance score of the file - a value between 0 and 1.

        - `text: optional string`

          The text that was retrieved from the file.

    - `ComputerCall object { id, call_id, pending_safety_checks, 4 more }`

      A tool call to a computer use tool. See the
      [computer use guide](/docs/guides/tools-computer-use) for more information.

      - `id: string`

        The unique ID of the computer call.

      - `call_id: string`

        An identifier used when responding to the tool call with output.

      - `pending_safety_checks: array of object { id, code, message }`

        The pending safety checks for the computer call.

        - `id: string`

          The ID of the pending safety check.

        - `code: optional string`

          The type of the pending safety check.

        - `message: optional string`

          Details about the pending safety check.

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the item. One of `in_progress`, `completed`, or
        `incomplete`. Populated when items are returned via API.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "computer_call"`

        The type of the computer call. Always `computer_call`.

        - `"computer_call"`

      - `action: optional ComputerAction`

        A click action.

        - `Click object { button, type, x, 2 more }`

          A click action.

          - `button: "left" or "right" or "wheel" or 2 more`

            Indicates which mouse button was pressed during the click. One of `left`, `right`, `wheel`, `back`, or `forward`.

            - `"left"`

            - `"right"`

            - `"wheel"`

            - `"back"`

            - `"forward"`

          - `type: "click"`

            Specifies the event type. For a click action, this property is always `click`.

            - `"click"`

          - `x: number`

            The x-coordinate where the click occurred.

          - `y: number`

            The y-coordinate where the click occurred.

          - `keys: optional array of string`

            The keys being held while clicking.

        - `DoubleClick object { keys, type, x, y }`

          A double click action.

          - `keys: array of string`

            The keys being held while double-clicking.

          - `type: "double_click"`

            Specifies the event type. For a double click action, this property is always set to `double_click`.

            - `"double_click"`

          - `x: number`

            The x-coordinate where the double click occurred.

          - `y: number`

            The y-coordinate where the double click occurred.

        - `Drag object { path, type, keys }`

          A drag action.

          - `path: array of object { x, y }`

            An array of coordinates representing the path of the drag action. Coordinates will appear as an array of objects, eg

            ```
            [
              { x: 100, y: 200 },
              { x: 200, y: 300 }
            ]
            ```

            - `x: number`

              The x-coordinate.

            - `y: number`

              The y-coordinate.

          - `type: "drag"`

            Specifies the event type. For a drag action, this property is always set to `drag`.

            - `"drag"`

          - `keys: optional array of string`

            The keys being held while dragging the mouse.

        - `Keypress object { keys, type }`

          A collection of keypresses the model would like to perform.

          - `keys: array of string`

            The combination of keys the model is requesting to be pressed. This is an array of strings, each representing a key.

          - `type: "keypress"`

            Specifies the event type. For a keypress action, this property is always set to `keypress`.

            - `"keypress"`

        - `Move object { type, x, y, keys }`

          A mouse move action.

          - `type: "move"`

            Specifies the event type. For a move action, this property is always set to `move`.

            - `"move"`

          - `x: number`

            The x-coordinate to move to.

          - `y: number`

            The y-coordinate to move to.

          - `keys: optional array of string`

            The keys being held while moving the mouse.

        - `Screenshot object { type }`

          A screenshot action.

          - `type: "screenshot"`

            Specifies the event type. For a screenshot action, this property is always set to `screenshot`.

            - `"screenshot"`

        - `Scroll object { scroll_x, scroll_y, type, 3 more }`

          A scroll action.

          - `scroll_x: number`

            The horizontal scroll distance.

          - `scroll_y: number`

            The vertical scroll distance.

          - `type: "scroll"`

            Specifies the event type. For a scroll action, this property is always set to `scroll`.

            - `"scroll"`

          - `x: number`

            The x-coordinate where the scroll occurred.

          - `y: number`

            The y-coordinate where the scroll occurred.

          - `keys: optional array of string`

            The keys being held while scrolling.

        - `Type object { text, type }`

          An action to type in text.

          - `text: string`

            The text to type.

          - `type: "type"`

            Specifies the event type. For a type action, this property is always set to `type`.

            - `"type"`

        - `Wait object { type }`

          A wait action.

          - `type: "wait"`

            Specifies the event type. For a wait action, this property is always set to `wait`.

            - `"wait"`

      - `actions: optional ComputerActionList`

        Flattened batched actions for `computer_use`. Each action includes an
        `type` discriminator and action-specific fields.

        - `Click object { button, type, x, 2 more }`

          A click action.

        - `DoubleClick object { keys, type, x, y }`

          A double click action.

        - `Drag object { path, type, keys }`

          A drag action.

        - `Keypress object { keys, type }`

          A collection of keypresses the model would like to perform.

        - `Move object { type, x, y, keys }`

          A mouse move action.

        - `Screenshot object { type }`

          A screenshot action.

        - `Scroll object { scroll_x, scroll_y, type, 3 more }`

          A scroll action.

        - `Type object { text, type }`

          An action to type in text.

        - `Wait object { type }`

          A wait action.

    - `ComputerCallOutput object { id, call_id, output, 4 more }`

      - `id: string`

        The unique ID of the computer call tool output.

      - `call_id: string`

        The ID of the computer tool call that produced the output.

      - `output: ResponseComputerToolCallOutputScreenshot`

        A computer screenshot image used with the computer use tool.

        - `type: "computer_screenshot"`

          Specifies the event type. For a computer screenshot, this property is
          always set to `computer_screenshot`.

          - `"computer_screenshot"`

        - `file_id: optional string`

          The identifier of an uploaded file that contains the screenshot.

        - `image_url: optional string`

          The URL of the screenshot image.

      - `status: "completed" or "incomplete" or "failed" or "in_progress"`

        The status of the message input. One of `in_progress`, `completed`, or
        `incomplete`. Populated when input items are returned via API.

        - `"completed"`

        - `"incomplete"`

        - `"failed"`

        - `"in_progress"`

      - `type: "computer_call_output"`

        The type of the computer tool call output. Always `computer_call_output`.

        - `"computer_call_output"`

      - `acknowledged_safety_checks: optional array of object { id, code, message }`

        The safety checks reported by the API that have been acknowledged by the
        developer.

        - `id: string`

          The ID of the pending safety check.

        - `code: optional string`

          The type of the pending safety check.

        - `message: optional string`

          Details about the pending safety check.

      - `created_by: optional string`

        The identifier of the actor that created the item.

    - `WebSearchCall object { id, action, status, type }`

      The results of a web search tool call. See the
      [web search guide](/docs/guides/tools-web-search) for more information.

      - `id: string`

        The unique ID of the web search tool call.

      - `action: object { query, type, queries, sources }  or object { type, url }  or object { pattern, type, url }`

        An object describing the specific action taken in this web search call.
        Includes details on how the model used the web (search, open_page, find_in_page).

        - `Search object { query, type, queries, sources }`

          Action type "search" - Performs a web search query.

          - `query: string`

            [DEPRECATED] The search query.

          - `type: "search"`

            The action type.

            - `"search"`

          - `queries: optional array of string`

            The search queries.

          - `sources: optional array of object { type, url }`

            The sources used in the search.

            - `type: "url"`

              The type of source. Always `url`.

              - `"url"`

            - `url: string`

              The URL of the source.

        - `OpenPage object { type, url }`

          Action type "open_page" - Opens a specific URL from search results.

          - `type: "open_page"`

            The action type. Always `open_page`.

            - `"open_page"`

          - `url: optional string`

            The URL opened by the model.

        - `FindInPage object { pattern, type, url }`

          Action type "find_in_page": Searches for a pattern within a loaded page.

          - `pattern: string`

            The pattern or text to search for within the page.

          - `type: "find_in_page"`

            The action type.

            - `"find_in_page"`

          - `url: string`

            The URL of the page searched for the pattern.

      - `status: "in_progress" or "searching" or "completed" or "failed"`

        The status of the web search tool call.

        - `"in_progress"`

        - `"searching"`

        - `"completed"`

        - `"failed"`

      - `type: "web_search_call"`

        The type of the web search tool call. Always `web_search_call`.

        - `"web_search_call"`

    - `FunctionCall object { id, arguments, call_id, 5 more }`

      - `id: string`

        The unique ID of the function tool call.

      - `arguments: string`

        A JSON string of the arguments to pass to the function.

      - `call_id: string`

        The unique ID of the function tool call generated by the model.

      - `name: string`

        The name of the function to run.

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the item. One of `in_progress`, `completed`, or
        `incomplete`. Populated when items are returned via API.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "function_call"`

        The type of the function tool call. Always `function_call`.

        - `"function_call"`

      - `created_by: optional string`

        The identifier of the actor that created the item.

      - `namespace: optional string`

        The namespace of the function to run.

    - `FunctionCallOutput object { id, call_id, output, 3 more }`

      - `id: string`

        The unique ID of the function call tool output.

      - `call_id: string`

        The unique ID of the function tool call generated by the model.

      - `output: string or array of ResponseInputText or ResponseInputImage or ResponseInputFile`

        The output from the function call generated by your code.
        Can be a string or an list of output content.

        - `StringOutput = string`

          A string of the output of the function call.

        - `OutputContentList = array of ResponseInputText or ResponseInputImage or ResponseInputFile`

          Text, image, or file output of the function call.

          - `ResponseInputText object { text, type }`

            A text input to the model.

          - `ResponseInputImage object { detail, type, file_id, image_url }`

            An image input to the model. Learn about [image inputs](/docs/guides/vision).

          - `ResponseInputFile object { type, detail, file_data, 3 more }`

            A file input to the model.

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the item. One of `in_progress`, `completed`, or
        `incomplete`. Populated when items are returned via API.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "function_call_output"`

        The type of the function tool call output. Always `function_call_output`.

        - `"function_call_output"`

      - `created_by: optional string`

        The identifier of the actor that created the item.

    - `ToolSearchCall object { id, arguments, call_id, 4 more }`

      - `id: string`

        The unique ID of the tool search call item.

      - `arguments: unknown`

        Arguments used for the tool search call.

      - `call_id: string`

        The unique ID of the tool search call generated by the model.

      - `execution: "server" or "client"`

        Whether tool search was executed by the server or by the client.

        - `"server"`

        - `"client"`

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the tool search call item that was recorded.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "tool_search_call"`

        The type of the item. Always `tool_search_call`.

        - `"tool_search_call"`

      - `created_by: optional string`

        The identifier of the actor that created the item.

    - `ToolSearchOutput object { id, call_id, execution, 4 more }`

      - `id: string`

        The unique ID of the tool search output item.

      - `call_id: string`

        The unique ID of the tool search call generated by the model.

      - `execution: "server" or "client"`

        Whether tool search was executed by the server or by the client.

        - `"server"`

        - `"client"`

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the tool search output item that was recorded.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `tools: array of object { name, parameters, strict, 3 more }  or object { type, vector_store_ids, filters, 2 more }  or object { type }  or 12 more`

        The loaded tool definitions returned by tool search.

        - `Function object { name, parameters, strict, 3 more }`

          Defines a function in your own code the model can choose to call. Learn more about [function calling](https://platform.openai.com/docs/guides/function-calling).

          - `name: string`

            The name of the function to call.

          - `parameters: map[unknown]`

            A JSON schema object describing the parameters of the function.

          - `strict: boolean`

            Whether to enforce strict parameter validation. Default `true`.

          - `type: "function"`

            The type of the function tool. Always `function`.

            - `"function"`

          - `defer_loading: optional boolean`

            Whether this function is deferred and loaded via tool search.

          - `description: optional string`

            A description of the function. Used by the model to determine whether or not to call the function.

        - `FileSearch object { type, vector_store_ids, filters, 2 more }`

          A tool that searches for relevant content from uploaded files. Learn more about the [file search tool](https://platform.openai.com/docs/guides/tools-file-search).

          - `type: "file_search"`

            The type of the file search tool. Always `file_search`.

            - `"file_search"`

          - `vector_store_ids: array of string`

            The IDs of the vector stores to search.

          - `filters: optional ComparisonFilter or CompoundFilter`

            A filter to apply.

            - `ComparisonFilter object { key, type, value }`

              A filter used to compare a specified attribute key to a given value using a defined comparison operation.

              - `key: string`

                The key to compare against the value.

              - `type: "eq" or "ne" or "gt" or 5 more`

                Specifies the comparison operator: `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `in`, `nin`.

                - `eq`: equals
                - `ne`: not equal
                - `gt`: greater than
                - `gte`: greater than or equal
                - `lt`: less than
                - `lte`: less than or equal
                - `in`: in
                - `nin`: not in

                - `"eq"`

                - `"ne"`

                - `"gt"`

                - `"gte"`

                - `"lt"`

                - `"lte"`

                - `"in"`

                - `"nin"`

              - `value: string or number or boolean or array of string or number`

                The value to compare against the attribute key; supports string, number, or boolean types.

                - `string`

                - `number`

                - `boolean`

                - `array of string or number`

                  - `string`

                  - `number`

            - `CompoundFilter object { filters, type }`

              Combine multiple filters using `and` or `or`.

              - `filters: array of ComparisonFilter or unknown`

                Array of filters to combine. Items can be `ComparisonFilter` or `CompoundFilter`.

                - `ComparisonFilter object { key, type, value }`

                  A filter used to compare a specified attribute key to a given value using a defined comparison operation.

                - `unknown`

              - `type: "and" or "or"`

                Type of operation: `and` or `or`.

                - `"and"`

                - `"or"`

          - `max_num_results: optional number`

            The maximum number of results to return. This number should be between 1 and 50 inclusive.

          - `ranking_options: optional object { hybrid_search, ranker, score_threshold }`

            Ranking options for search.

            - `hybrid_search: optional object { embedding_weight, text_weight }`

              Weights that control how reciprocal rank fusion balances semantic embedding matches versus sparse keyword matches when hybrid search is enabled.

              - `embedding_weight: number`

                The weight of the embedding in the reciprocal ranking fusion.

              - `text_weight: number`

                The weight of the text in the reciprocal ranking fusion.

            - `ranker: optional "auto" or "default-2024-11-15"`

              The ranker to use for the file search.

              - `"auto"`

              - `"default-2024-11-15"`

            - `score_threshold: optional number`

              The score threshold for the file search, a number between 0 and 1. Numbers closer to 1 will attempt to return only the most relevant results, but may return fewer results.

        - `Computer object { type }`

          A tool that controls a virtual computer. Learn more about the [computer tool](https://platform.openai.com/docs/guides/tools-computer-use).

          - `type: "computer"`

            The type of the computer tool. Always `computer`.

            - `"computer"`

        - `ComputerUsePreview object { display_height, display_width, environment, type }`

          A tool that controls a virtual computer. Learn more about the [computer tool](https://platform.openai.com/docs/guides/tools-computer-use).

          - `display_height: number`

            The height of the computer display.

          - `display_width: number`

            The width of the computer display.

          - `environment: "windows" or "mac" or "linux" or 2 more`

            The type of computer environment to control.

            - `"windows"`

            - `"mac"`

            - `"linux"`

            - `"ubuntu"`

            - `"browser"`

          - `type: "computer_use_preview"`

            The type of the computer use tool. Always `computer_use_preview`.

            - `"computer_use_preview"`

        - `WebSearch object { type, filters, search_context_size, user_location }`

          Search the Internet for sources related to the prompt. Learn more about the
          [web search tool](/docs/guides/tools-web-search).

          - `type: "web_search" or "web_search_2025_08_26"`

            The type of the web search tool. One of `web_search` or `web_search_2025_08_26`.

            - `"web_search"`

            - `"web_search_2025_08_26"`

          - `filters: optional object { allowed_domains }`

            Filters for the search.

            - `allowed_domains: optional array of string`

              Allowed domains for the search. If not provided, all domains are allowed.
              Subdomains of the provided domains are allowed as well.

              Example: `["pubmed.ncbi.nlm.nih.gov"]`

          - `search_context_size: optional "low" or "medium" or "high"`

            High level guidance for the amount of context window space to use for the search. One of `low`, `medium`, or `high`. `medium` is the default.

            - `"low"`

            - `"medium"`

            - `"high"`

          - `user_location: optional object { city, country, region, 2 more }`

            The approximate location of the user.

            - `city: optional string`

              Free text input for the city of the user, e.g. `San Francisco`.

            - `country: optional string`

              The two-letter [ISO country code](https://en.wikipedia.org/wiki/ISO_3166-1) of the user, e.g. `US`.

            - `region: optional string`

              Free text input for the region of the user, e.g. `California`.

            - `timezone: optional string`

              The [IANA timezone](https://timeapi.io/documentation/iana-timezones) of the user, e.g. `America/Los_Angeles`.

            - `type: optional "approximate"`

              The type of location approximation. Always `approximate`.

              - `"approximate"`

        - `Mcp object { server_label, type, allowed_tools, 7 more }`

          Give the model access to additional tools via remote Model Context Protocol
          (MCP) servers. [Learn more about MCP](/docs/guides/tools-remote-mcp).

          - `server_label: string`

            A label for this MCP server, used to identify it in tool calls.

          - `type: "mcp"`

            The type of the MCP tool. Always `mcp`.

            - `"mcp"`

          - `allowed_tools: optional array of string or object { read_only, tool_names }`

            List of allowed tool names or a filter object.

            - `McpAllowedTools = array of string`

              A string array of allowed tool names

            - `McpToolFilter object { read_only, tool_names }`

              A filter object to specify which tools are allowed.

              - `read_only: optional boolean`

                Indicates whether or not a tool modifies data or is read-only. If an
                MCP server is [annotated with `readOnlyHint`](https://modelcontextprotocol.io/specification/2025-06-18/schema#toolannotations-readonlyhint),
                it will match this filter.

              - `tool_names: optional array of string`

                List of allowed tool names.

          - `authorization: optional string`

            An OAuth access token that can be used with a remote MCP server, either
            with a custom MCP server URL or a service connector. Your application
            must handle the OAuth authorization flow and provide the token here.

          - `connector_id: optional "connector_dropbox" or "connector_gmail" or "connector_googlecalendar" or 5 more`

            Identifier for service connectors, like those available in ChatGPT. One of
            `server_url` or `connector_id` must be provided. Learn more about service
            connectors [here](/docs/guides/tools-remote-mcp#connectors).

            Currently supported `connector_id` values are:

            - Dropbox: `connector_dropbox`
            - Gmail: `connector_gmail`
            - Google Calendar: `connector_googlecalendar`
            - Google Drive: `connector_googledrive`
            - Microsoft Teams: `connector_microsoftteams`
            - Outlook Calendar: `connector_outlookcalendar`
            - Outlook Email: `connector_outlookemail`
            - SharePoint: `connector_sharepoint`

            - `"connector_dropbox"`

            - `"connector_gmail"`

            - `"connector_googlecalendar"`

            - `"connector_googledrive"`

            - `"connector_microsoftteams"`

            - `"connector_outlookcalendar"`

            - `"connector_outlookemail"`

            - `"connector_sharepoint"`

          - `defer_loading: optional boolean`

            Whether this MCP tool is deferred and discovered via tool search.

          - `headers: optional map[string]`

            Optional HTTP headers to send to the MCP server. Use for authentication
            or other purposes.

          - `require_approval: optional object { always, never }  or "always" or "never"`

            Specify which of the MCP server's tools require approval.

            - `McpToolApprovalFilter object { always, never }`

              Specify which of the MCP server's tools require approval. Can be
              `always`, `never`, or a filter object associated with tools
              that require approval.

              - `always: optional object { read_only, tool_names }`

                A filter object to specify which tools are allowed.

                - `read_only: optional boolean`

                  Indicates whether or not a tool modifies data or is read-only. If an
                  MCP server is [annotated with `readOnlyHint`](https://modelcontextprotocol.io/specification/2025-06-18/schema#toolannotations-readonlyhint),
                  it will match this filter.

                - `tool_names: optional array of string`

                  List of allowed tool names.

              - `never: optional object { read_only, tool_names }`

                A filter object to specify which tools are allowed.

                - `read_only: optional boolean`

                  Indicates whether or not a tool modifies data or is read-only. If an
                  MCP server is [annotated with `readOnlyHint`](https://modelcontextprotocol.io/specification/2025-06-18/schema#toolannotations-readonlyhint),
                  it will match this filter.

                - `tool_names: optional array of string`

                  List of allowed tool names.

            - `McpToolApprovalSetting = "always" or "never"`

              Specify a single approval policy for all tools. One of `always` or
              `never`. When set to `always`, all tools will require approval. When
              set to `never`, all tools will not require approval.

              - `"always"`

              - `"never"`

          - `server_description: optional string`

            Optional description of the MCP server, used to provide more context.

          - `server_url: optional string`

            The URL for the MCP server. One of `server_url` or `connector_id` must be
            provided.

        - `CodeInterpreter object { container, type }`

          A tool that runs Python code to help generate a response to a prompt.

          - `container: string or object { type, file_ids, memory_limit, network_policy }`

            The code interpreter container. Can be a container ID or an object that
            specifies uploaded file IDs to make available to your code, along with an
            optional `memory_limit` setting.

            - `string`

              The container ID.

            - `CodeInterpreterToolAuto object { type, file_ids, memory_limit, network_policy }`

              Configuration for a code interpreter container. Optionally specify the IDs of the files to run the code on.

              - `type: "auto"`

                Always `auto`.

                - `"auto"`

              - `file_ids: optional array of string`

                An optional list of uploaded files to make available to your code.

              - `memory_limit: optional "1g" or "4g" or "16g" or "64g"`

                The memory limit for the code interpreter container.

                - `"1g"`

                - `"4g"`

                - `"16g"`

                - `"64g"`

              - `network_policy: optional ContainerNetworkPolicyDisabled or ContainerNetworkPolicyAllowlist`

                Network access policy for the container.

                - `ContainerNetworkPolicyDisabled object { type }`

                  - `type: "disabled"`

                    Disable outbound network access. Always `disabled`.

                    - `"disabled"`

                - `ContainerNetworkPolicyAllowlist object { allowed_domains, type, domain_secrets }`

                  - `allowed_domains: array of string`

                    A list of allowed domains when type is `allowlist`.

                  - `type: "allowlist"`

                    Allow outbound network access only to specified domains. Always `allowlist`.

                    - `"allowlist"`

                  - `domain_secrets: optional array of ContainerNetworkPolicyDomainSecret`

                    Optional domain-scoped secrets for allowlisted domains.

                    - `domain: string`

                      The domain associated with the secret.

                    - `name: string`

                      The name of the secret to inject for the domain.

                    - `value: string`

                      The secret value to inject for the domain.

          - `type: "code_interpreter"`

            The type of the code interpreter tool. Always `code_interpreter`.

            - `"code_interpreter"`

        - `ImageGeneration object { type, action, background, 9 more }`

          A tool that generates images using the GPT image models.

          - `type: "image_generation"`

            The type of the image generation tool. Always `image_generation`.

            - `"image_generation"`

          - `action: optional "generate" or "edit" or "auto"`

            Whether to generate a new image or edit an existing image. Default: `auto`.

            - `"generate"`

            - `"edit"`

            - `"auto"`

          - `background: optional "transparent" or "opaque" or "auto"`

            Background type for the generated image. One of `transparent`,
            `opaque`, or `auto`. Default: `auto`.

            - `"transparent"`

            - `"opaque"`

            - `"auto"`

          - `input_fidelity: optional "high" or "low"`

            Control how much effort the model will exert to match the style and features, especially facial features, of input images. This parameter is only supported for `gpt-image-1` and `gpt-image-1.5` and later models, unsupported for `gpt-image-1-mini`. Supports `high` and `low`. Defaults to `low`.

            - `"high"`

            - `"low"`

          - `input_image_mask: optional object { file_id, image_url }`

            Optional mask for inpainting. Contains `image_url`
            (string, optional) and `file_id` (string, optional).

            - `file_id: optional string`

              File ID for the mask image.

            - `image_url: optional string`

              Base64-encoded mask image.

          - `model: optional string or "gpt-image-1" or "gpt-image-1-mini" or "gpt-image-1.5"`

            The image generation model to use. Default: `gpt-image-1`.

            - `string`

            - `"gpt-image-1" or "gpt-image-1-mini" or "gpt-image-1.5"`

              The image generation model to use. Default: `gpt-image-1`.

              - `"gpt-image-1"`

              - `"gpt-image-1-mini"`

              - `"gpt-image-1.5"`

          - `moderation: optional "auto" or "low"`

            Moderation level for the generated image. Default: `auto`.

            - `"auto"`

            - `"low"`

          - `output_compression: optional number`

            Compression level for the output image. Default: 100.

          - `output_format: optional "png" or "webp" or "jpeg"`

            The output format of the generated image. One of `png`, `webp`, or
            `jpeg`. Default: `png`.

            - `"png"`

            - `"webp"`

            - `"jpeg"`

          - `partial_images: optional number`

            Number of partial images to generate in streaming mode, from 0 (default value) to 3.

          - `quality: optional "low" or "medium" or "high" or "auto"`

            The quality of the generated image. One of `low`, `medium`, `high`,
            or `auto`. Default: `auto`.

            - `"low"`

            - `"medium"`

            - `"high"`

            - `"auto"`

          - `size: optional string or "1024x1024" or "1024x1536" or "1536x1024" or "auto"`

            The size of the generated images. For `gpt-image-2` and `gpt-image-2-2026-04-21`, arbitrary resolutions are supported as `WIDTHxHEIGHT` strings, for example `1536x864`. Width and height must both be divisible by 16 and the requested aspect ratio must be between 1:3 and 3:1. Resolutions above `2560x1440` are experimental, and the maximum supported resolution is `3840x2160`. The requested size must also satisfy the model's current pixel and edge limits. The standard sizes `1024x1024`, `1536x1024`, and `1024x1536` are supported by the GPT image models; `auto` is supported for models that allow automatic sizing. For `dall-e-2`, use one of `256x256`, `512x512`, or `1024x1024`. For `dall-e-3`, use one of `1024x1024`, `1792x1024`, or `1024x1792`.

            - `string`

            - `"1024x1024" or "1024x1536" or "1536x1024" or "auto"`

              The size of the generated images. For `gpt-image-2` and `gpt-image-2-2026-04-21`, arbitrary resolutions are supported as `WIDTHxHEIGHT` strings, for example `1536x864`. Width and height must both be divisible by 16 and the requested aspect ratio must be between 1:3 and 3:1. Resolutions above `2560x1440` are experimental, and the maximum supported resolution is `3840x2160`. The requested size must also satisfy the model's current pixel and edge limits. The standard sizes `1024x1024`, `1536x1024`, and `1024x1536` are supported by the GPT image models; `auto` is supported for models that allow automatic sizing. For `dall-e-2`, use one of `256x256`, `512x512`, or `1024x1024`. For `dall-e-3`, use one of `1024x1024`, `1792x1024`, or `1024x1792`.

              - `"1024x1024"`

              - `"1024x1536"`

              - `"1536x1024"`

              - `"auto"`

        - `LocalShell object { type }`

          A tool that allows the model to execute shell commands in a local environment.

          - `type: "local_shell"`

            The type of the local shell tool. Always `local_shell`.

            - `"local_shell"`

        - `Shell object { type, environment }`

          A tool that allows the model to execute shell commands.

          - `type: "shell"`

            The type of the shell tool. Always `shell`.

            - `"shell"`

          - `environment: optional ContainerAuto or LocalEnvironment or ContainerReference`

            - `ContainerAuto object { type, file_ids, memory_limit, 2 more }`

              - `type: "container_auto"`

                Automatically creates a container for this request

                - `"container_auto"`

              - `file_ids: optional array of string`

                An optional list of uploaded files to make available to your code.

              - `memory_limit: optional "1g" or "4g" or "16g" or "64g"`

                The memory limit for the container.

                - `"1g"`

                - `"4g"`

                - `"16g"`

                - `"64g"`

              - `network_policy: optional ContainerNetworkPolicyDisabled or ContainerNetworkPolicyAllowlist`

                Network access policy for the container.

                - `ContainerNetworkPolicyDisabled object { type }`

                - `ContainerNetworkPolicyAllowlist object { allowed_domains, type, domain_secrets }`

              - `skills: optional array of SkillReference or InlineSkill`

                An optional list of skills referenced by id or inline data.

                - `SkillReference object { skill_id, type, version }`

                  - `skill_id: string`

                    The ID of the referenced skill.

                  - `type: "skill_reference"`

                    References a skill created with the /v1/skills endpoint.

                    - `"skill_reference"`

                  - `version: optional string`

                    Optional skill version. Use a positive integer or 'latest'. Omit for default.

                - `InlineSkill object { description, name, source, type }`

                  - `description: string`

                    The description of the skill.

                  - `name: string`

                    The name of the skill.

                  - `source: InlineSkillSource`

                    Inline skill payload

                    - `data: string`

                      Base64-encoded skill zip bundle.

                    - `media_type: "application/zip"`

                      The media type of the inline skill payload. Must be `application/zip`.

                      - `"application/zip"`

                    - `type: "base64"`

                      The type of the inline skill source. Must be `base64`.

                      - `"base64"`

                  - `type: "inline"`

                    Defines an inline skill for this request.

                    - `"inline"`

            - `LocalEnvironment object { type, skills }`

              - `type: "local"`

                Use a local computer environment.

                - `"local"`

              - `skills: optional array of LocalSkill`

                An optional list of skills.

                - `description: string`

                  The description of the skill.

                - `name: string`

                  The name of the skill.

                - `path: string`

                  The path to the directory containing the skill.

            - `ContainerReference object { container_id, type }`

              - `container_id: string`

                The ID of the referenced container.

              - `type: "container_reference"`

                References a container created with the /v1/containers endpoint

                - `"container_reference"`

        - `Custom object { name, type, defer_loading, 2 more }`

          A custom tool that processes input using a specified format. Learn more about   [custom tools](/docs/guides/function-calling#custom-tools)

          - `name: string`

            The name of the custom tool, used to identify it in tool calls.

          - `type: "custom"`

            The type of the custom tool. Always `custom`.

            - `"custom"`

          - `defer_loading: optional boolean`

            Whether this tool should be deferred and discovered via tool search.

          - `description: optional string`

            Optional description of the custom tool, used to provide more context.

          - `format: optional CustomToolInputFormat`

            The input format for the custom tool. Default is unconstrained text.

            - `Text object { type }`

              Unconstrained free-form text.

              - `type: "text"`

                Unconstrained text format. Always `text`.

                - `"text"`

            - `Grammar object { definition, syntax, type }`

              A grammar defined by the user.

              - `definition: string`

                The grammar definition.

              - `syntax: "lark" or "regex"`

                The syntax of the grammar definition. One of `lark` or `regex`.

                - `"lark"`

                - `"regex"`

              - `type: "grammar"`

                Grammar format. Always `grammar`.

                - `"grammar"`

        - `Namespace object { description, name, tools, type }`

          Groups function/custom tools under a shared namespace.

          - `description: string`

            A description of the namespace shown to the model.

          - `name: string`

            The namespace name used in tool calls (for example, `crm`).

          - `tools: array of object { name, type, defer_loading, 3 more }  or object { name, type, defer_loading, 2 more }`

            The function/custom tools available inside this namespace.

            - `Function object { name, type, defer_loading, 3 more }`

              - `name: string`

              - `type: "function"`

                - `"function"`

              - `defer_loading: optional boolean`

                Whether this function should be deferred and discovered via tool search.

              - `description: optional string`

              - `parameters: optional unknown`

              - `strict: optional boolean`

            - `Custom object { name, type, defer_loading, 2 more }`

              A custom tool that processes input using a specified format. Learn more about   [custom tools](/docs/guides/function-calling#custom-tools)

              - `name: string`

                The name of the custom tool, used to identify it in tool calls.

              - `type: "custom"`

                The type of the custom tool. Always `custom`.

                - `"custom"`

              - `defer_loading: optional boolean`

                Whether this tool should be deferred and discovered via tool search.

              - `description: optional string`

                Optional description of the custom tool, used to provide more context.

              - `format: optional CustomToolInputFormat`

                The input format for the custom tool. Default is unconstrained text.

          - `type: "namespace"`

            The type of the tool. Always `namespace`.

            - `"namespace"`

        - `ToolSearch object { type, description, execution, parameters }`

          Hosted or BYOT tool search configuration for deferred tools.

          - `type: "tool_search"`

            The type of the tool. Always `tool_search`.

            - `"tool_search"`

          - `description: optional string`

            Description shown to the model for a client-executed tool search tool.

          - `execution: optional "server" or "client"`

            Whether tool search is executed by the server or by the client.

            - `"server"`

            - `"client"`

          - `parameters: optional unknown`

            Parameter schema for a client-executed tool search tool.

        - `WebSearchPreview object { type, search_content_types, search_context_size, user_location }`

          This tool searches the web for relevant results to use in a response. Learn more about the [web search tool](https://platform.openai.com/docs/guides/tools-web-search).

          - `type: "web_search_preview" or "web_search_preview_2025_03_11"`

            The type of the web search tool. One of `web_search_preview` or `web_search_preview_2025_03_11`.

            - `"web_search_preview"`

            - `"web_search_preview_2025_03_11"`

          - `search_content_types: optional array of "text" or "image"`

            - `"text"`

            - `"image"`

          - `search_context_size: optional "low" or "medium" or "high"`

            High level guidance for the amount of context window space to use for the search. One of `low`, `medium`, or `high`. `medium` is the default.

            - `"low"`

            - `"medium"`

            - `"high"`

          - `user_location: optional object { type, city, country, 2 more }`

            The user's location.

            - `type: "approximate"`

              The type of location approximation. Always `approximate`.

              - `"approximate"`

            - `city: optional string`

              Free text input for the city of the user, e.g. `San Francisco`.

            - `country: optional string`

              The two-letter [ISO country code](https://en.wikipedia.org/wiki/ISO_3166-1) of the user, e.g. `US`.

            - `region: optional string`

              Free text input for the region of the user, e.g. `California`.

            - `timezone: optional string`

              The [IANA timezone](https://timeapi.io/documentation/iana-timezones) of the user, e.g. `America/Los_Angeles`.

        - `ApplyPatch object { type }`

          Allows the assistant to create, delete, or update files using unified diffs.

          - `type: "apply_patch"`

            The type of the tool. Always `apply_patch`.

            - `"apply_patch"`

      - `type: "tool_search_output"`

        The type of the item. Always `tool_search_output`.

        - `"tool_search_output"`

      - `created_by: optional string`

        The identifier of the actor that created the item.

    - `Reasoning object { id, summary, type, 3 more }`

      A description of the chain of thought used by a reasoning model while generating
      a response. Be sure to include these items in your `input` to the Responses API
      for subsequent turns of a conversation if you are manually
      [managing context](/docs/guides/conversation-state).

      - `id: string`

        The unique identifier of the reasoning content.

      - `summary: array of SummaryTextContent`

        Reasoning summary content.

        - `text: string`

          A summary of the reasoning output from the model so far.

        - `type: "summary_text"`

          The type of the object. Always `summary_text`.

          - `"summary_text"`

      - `type: "reasoning"`

        The type of the object. Always `reasoning`.

        - `"reasoning"`

      - `content: optional array of object { text, type }`

        Reasoning text content.

        - `text: string`

          The reasoning text from the model.

        - `type: "reasoning_text"`

          The type of the reasoning text. Always `reasoning_text`.

          - `"reasoning_text"`

      - `encrypted_content: optional string`

        The encrypted content of the reasoning item - populated when a response is
        generated with `reasoning.encrypted_content` in the `include` parameter.

      - `status: optional "in_progress" or "completed" or "incomplete"`

        The status of the item. One of `in_progress`, `completed`, or
        `incomplete`. Populated when items are returned via API.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

    - `Compaction object { id, encrypted_content, type, created_by }`

      A compaction item generated by the [`v1/responses/compact` API](/docs/api-reference/responses/compact).

      - `id: string`

        The unique ID of the compaction item.

      - `encrypted_content: string`

        The encrypted content that was produced by compaction.

      - `type: "compaction"`

        The type of the item. Always `compaction`.

        - `"compaction"`

      - `created_by: optional string`

        The identifier of the actor that created the item.

    - `ImageGenerationCall object { id, result, status, type }`

      An image generation request made by the model.

      - `id: string`

        The unique ID of the image generation call.

      - `result: string`

        The generated image encoded in base64.

      - `status: "in_progress" or "completed" or "generating" or "failed"`

        The status of the image generation call.

        - `"in_progress"`

        - `"completed"`

        - `"generating"`

        - `"failed"`

      - `type: "image_generation_call"`

        The type of the image generation call. Always `image_generation_call`.

        - `"image_generation_call"`

    - `CodeInterpreterCall object { id, code, container_id, 3 more }`

      A tool call to run code.

      - `id: string`

        The unique ID of the code interpreter tool call.

      - `code: string`

        The code to run, or null if not available.

      - `container_id: string`

        The ID of the container used to run the code.

      - `outputs: array of object { logs, type }  or object { type, url }`

        The outputs generated by the code interpreter, such as logs or images.
        Can be null if no outputs are available.

        - `Logs object { logs, type }`

          The logs output from the code interpreter.

          - `logs: string`

            The logs output from the code interpreter.

          - `type: "logs"`

            The type of the output. Always `logs`.

            - `"logs"`

        - `Image object { type, url }`

          The image output from the code interpreter.

          - `type: "image"`

            The type of the output. Always `image`.

            - `"image"`

          - `url: string`

            The URL of the image output from the code interpreter.

      - `status: "in_progress" or "completed" or "incomplete" or 2 more`

        The status of the code interpreter tool call. Valid values are `in_progress`, `completed`, `incomplete`, `interpreting`, and `failed`.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

        - `"interpreting"`

        - `"failed"`

      - `type: "code_interpreter_call"`

        The type of the code interpreter tool call. Always `code_interpreter_call`.

        - `"code_interpreter_call"`

    - `LocalShellCall object { id, action, call_id, 2 more }`

      A tool call to run a command on the local shell.

      - `id: string`

        The unique ID of the local shell call.

      - `action: object { command, env, type, 3 more }`

        Execute a shell command on the server.

        - `command: array of string`

          The command to run.

        - `env: map[string]`

          Environment variables to set for the command.

        - `type: "exec"`

          The type of the local shell action. Always `exec`.

          - `"exec"`

        - `timeout_ms: optional number`

          Optional timeout in milliseconds for the command.

        - `user: optional string`

          Optional user to run the command as.

        - `working_directory: optional string`

          Optional working directory to run the command in.

      - `call_id: string`

        The unique ID of the local shell tool call generated by the model.

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the local shell call.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "local_shell_call"`

        The type of the local shell call. Always `local_shell_call`.

        - `"local_shell_call"`

    - `LocalShellCallOutput object { id, output, type, status }`

      The output of a local shell tool call.

      - `id: string`

        The unique ID of the local shell tool call generated by the model.

      - `output: string`

        A JSON string of the output of the local shell tool call.

      - `type: "local_shell_call_output"`

        The type of the local shell tool call output. Always `local_shell_call_output`.

        - `"local_shell_call_output"`

      - `status: optional "in_progress" or "completed" or "incomplete"`

        The status of the item. One of `in_progress`, `completed`, or `incomplete`.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

    - `ShellCall object { id, action, call_id, 4 more }`

      A tool call that executes one or more shell commands in a managed environment.

      - `id: string`

        The unique ID of the shell tool call. Populated when this item is returned via API.

      - `action: object { commands, max_output_length, timeout_ms }`

        The shell commands and limits that describe how to run the tool call.

        - `commands: array of string`

        - `max_output_length: number`

          Optional maximum number of characters to return from each command.

        - `timeout_ms: number`

          Optional timeout in milliseconds for the commands.

      - `call_id: string`

        The unique ID of the shell tool call generated by the model.

      - `environment: ResponseLocalEnvironment or ResponseContainerReference`

        Represents the use of a local environment to perform shell actions.

        - `ResponseLocalEnvironment object { type }`

          Represents the use of a local environment to perform shell actions.

          - `type: "local"`

            The environment type. Always `local`.

            - `"local"`

        - `ResponseContainerReference object { container_id, type }`

          Represents a container created with /v1/containers.

          - `container_id: string`

          - `type: "container_reference"`

            The environment type. Always `container_reference`.

            - `"container_reference"`

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the shell call. One of `in_progress`, `completed`, or `incomplete`.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "shell_call"`

        The type of the item. Always `shell_call`.

        - `"shell_call"`

      - `created_by: optional string`

        The ID of the entity that created this tool call.

    - `ShellCallOutput object { id, call_id, max_output_length, 4 more }`

      The output of a shell tool call that was emitted.

      - `id: string`

        The unique ID of the shell call output. Populated when this item is returned via API.

      - `call_id: string`

        The unique ID of the shell tool call generated by the model.

      - `max_output_length: number`

        The maximum length of the shell command output. This is generated by the model and should be passed back with the raw output.

      - `output: array of object { outcome, stderr, stdout, created_by }`

        An array of shell call output contents

        - `outcome: object { type }  or object { exit_code, type }`

          Represents either an exit outcome (with an exit code) or a timeout outcome for a shell call output chunk.

          - `Timeout object { type }`

            Indicates that the shell call exceeded its configured time limit.

            - `type: "timeout"`

              The outcome type. Always `timeout`.

              - `"timeout"`

          - `Exit object { exit_code, type }`

            Indicates that the shell commands finished and returned an exit code.

            - `exit_code: number`

              Exit code from the shell process.

            - `type: "exit"`

              The outcome type. Always `exit`.

              - `"exit"`

        - `stderr: string`

          The standard error output that was captured.

        - `stdout: string`

          The standard output that was captured.

        - `created_by: optional string`

          The identifier of the actor that created the item.

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the shell call output. One of `in_progress`, `completed`, or `incomplete`.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "shell_call_output"`

        The type of the shell call output. Always `shell_call_output`.

        - `"shell_call_output"`

      - `created_by: optional string`

        The identifier of the actor that created the item.

    - `ApplyPatchCall object { id, call_id, operation, 3 more }`

      A tool call that applies file diffs by creating, deleting, or updating files.

      - `id: string`

        The unique ID of the apply patch tool call. Populated when this item is returned via API.

      - `call_id: string`

        The unique ID of the apply patch tool call generated by the model.

      - `operation: object { diff, path, type }  or object { path, type }  or object { diff, path, type }`

        One of the create_file, delete_file, or update_file operations applied via apply_patch.

        - `CreateFile object { diff, path, type }`

          Instruction describing how to create a file via the apply_patch tool.

          - `diff: string`

            Diff to apply.

          - `path: string`

            Path of the file to create.

          - `type: "create_file"`

            Create a new file with the provided diff.

            - `"create_file"`

        - `DeleteFile object { path, type }`

          Instruction describing how to delete a file via the apply_patch tool.

          - `path: string`

            Path of the file to delete.

          - `type: "delete_file"`

            Delete the specified file.

            - `"delete_file"`

        - `UpdateFile object { diff, path, type }`

          Instruction describing how to update a file via the apply_patch tool.

          - `diff: string`

            Diff to apply.

          - `path: string`

            Path of the file to update.

          - `type: "update_file"`

            Update an existing file with the provided diff.

            - `"update_file"`

      - `status: "in_progress" or "completed"`

        The status of the apply patch tool call. One of `in_progress` or `completed`.

        - `"in_progress"`

        - `"completed"`

      - `type: "apply_patch_call"`

        The type of the item. Always `apply_patch_call`.

        - `"apply_patch_call"`

      - `created_by: optional string`

        The ID of the entity that created this tool call.

    - `ApplyPatchCallOutput object { id, call_id, status, 3 more }`

      The output emitted by an apply patch tool call.

      - `id: string`

        The unique ID of the apply patch tool call output. Populated when this item is returned via API.

      - `call_id: string`

        The unique ID of the apply patch tool call generated by the model.

      - `status: "completed" or "failed"`

        The status of the apply patch tool call output. One of `completed` or `failed`.

        - `"completed"`

        - `"failed"`

      - `type: "apply_patch_call_output"`

        The type of the item. Always `apply_patch_call_output`.

        - `"apply_patch_call_output"`

      - `created_by: optional string`

        The ID of the entity that created this tool call output.

      - `output: optional string`

        Optional textual output returned by the apply patch tool.

    - `McpListTools object { id, server_label, tools, 2 more }`

      A list of tools available on an MCP server.

      - `id: string`

        The unique ID of the list.

      - `server_label: string`

        The label of the MCP server.

      - `tools: array of object { input_schema, name, annotations, description }`

        The tools available on the server.

        - `input_schema: unknown`

          The JSON schema describing the tool's input.

        - `name: string`

          The name of the tool.

        - `annotations: optional unknown`

          Additional annotations about the tool.

        - `description: optional string`

          The description of the tool.

      - `type: "mcp_list_tools"`

        The type of the item. Always `mcp_list_tools`.

        - `"mcp_list_tools"`

      - `error: optional string`

        Error message if the server could not list tools.

    - `McpApprovalRequest object { id, arguments, name, 2 more }`

      A request for human approval of a tool invocation.

      - `id: string`

        The unique ID of the approval request.

      - `arguments: string`

        A JSON string of arguments for the tool.

      - `name: string`

        The name of the tool to run.

      - `server_label: string`

        The label of the MCP server making the request.

      - `type: "mcp_approval_request"`

        The type of the item. Always `mcp_approval_request`.

        - `"mcp_approval_request"`

    - `McpApprovalResponse object { id, approval_request_id, approve, 2 more }`

      A response to an MCP approval request.

      - `id: string`

        The unique ID of the approval response

      - `approval_request_id: string`

        The ID of the approval request being answered.

      - `approve: boolean`

        Whether the request was approved.

      - `type: "mcp_approval_response"`

        The type of the item. Always `mcp_approval_response`.

        - `"mcp_approval_response"`

      - `reason: optional string`

        Optional reason for the decision.

    - `McpCall object { id, arguments, name, 6 more }`

      An invocation of a tool on an MCP server.

      - `id: string`

        The unique ID of the tool call.

      - `arguments: string`

        A JSON string of the arguments passed to the tool.

      - `name: string`

        The name of the tool that was run.

      - `server_label: string`

        The label of the MCP server running the tool.

      - `type: "mcp_call"`

        The type of the item. Always `mcp_call`.

        - `"mcp_call"`

      - `approval_request_id: optional string`

        Unique identifier for the MCP tool call approval request.
        Include this value in a subsequent `mcp_approval_response` input to approve or reject the corresponding tool call.

      - `error: optional string`

        The error from the tool call, if any.

      - `output: optional string`

        The output from the tool call.

      - `status: optional "in_progress" or "completed" or "incomplete" or 2 more`

        The status of the tool call. One of `in_progress`, `completed`, `incomplete`, `calling`, or `failed`.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

        - `"calling"`

        - `"failed"`

    - `CustomToolCall object { id, call_id, input, 5 more }`

      - `id: string`

        The unique ID of the custom tool call item.

      - `call_id: string`

        An identifier used to map this custom tool call to a tool call output.

      - `input: string`

        The input for the custom tool call generated by the model.

      - `name: string`

        The name of the custom tool being called.

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the item. One of `in_progress`, `completed`, or
        `incomplete`. Populated when items are returned via API.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "custom_tool_call"`

        The type of the custom tool call. Always `custom_tool_call`.

        - `"custom_tool_call"`

      - `created_by: optional string`

        The identifier of the actor that created the item.

      - `namespace: optional string`

        The namespace of the custom tool being called.

    - `CustomToolCallOutput object { id, call_id, output, 3 more }`

      - `id: string`

        The unique ID of the custom tool call output item.

      - `call_id: string`

        The call ID, used to map this custom tool call output to a custom tool call.

      - `output: string or array of ResponseInputText or ResponseInputImage or ResponseInputFile`

        The output from the custom tool call generated by your code.
        Can be a string or an list of output content.

        - `StringOutput = string`

          A string of the output of the custom tool call.

        - `OutputContentList = array of ResponseInputText or ResponseInputImage or ResponseInputFile`

          Text, image, or file output of the custom tool call.

          - `ResponseInputText object { text, type }`

            A text input to the model.

          - `ResponseInputImage object { detail, type, file_id, image_url }`

            An image input to the model. Learn about [image inputs](/docs/guides/vision).

          - `ResponseInputFile object { type, detail, file_data, 3 more }`

            A file input to the model.

      - `status: "in_progress" or "completed" or "incomplete"`

        The status of the item. One of `in_progress`, `completed`, or
        `incomplete`. Populated when items are returned via API.

        - `"in_progress"`

        - `"completed"`

        - `"incomplete"`

      - `type: "custom_tool_call_output"`

        The type of the custom tool call output. Always `custom_tool_call_output`.

        - `"custom_tool_call_output"`

      - `created_by: optional string`

        The identifier of the actor that created the item.

  - `first_id: string`

    The ID of the first item in the list.

  - `has_more: boolean`

    Whether there are more items available.

  - `last_id: string`

    The ID of the last item in the list.

  - `object: "list"`

    The type of object returned, must be `list`.

    - `"list"`

### Example

```http
curl https://api.openai.com/v1/responses/$RESPONSE_ID/input_items \
    -H "Authorization: Bearer $OPENAI_API_KEY"
```

#### Response

```json
{
  "data": [
    {
      "id": "id",
      "content": [
        {
          "text": "text",
          "type": "input_text"
        }
      ],
      "role": "user",
      "type": "message",
      "status": "in_progress"
    }
  ],
  "first_id": "first_id",
  "has_more": true,
  "last_id": "last_id",
  "object": "list"
}
```

### Example

```http
curl https://api.openai.com/v1/responses/resp_abc123/input_items \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY"
```

#### Response

```json
{
  "object": "list",
  "data": [
    {
      "id": "msg_abc123",
      "type": "message",
      "role": "user",
      "content": [
        {
          "type": "input_text",
          "text": "Tell me a three sentence bedtime story about a unicorn."
        }
      ]
    }
  ],
  "first_id": "msg_abc123",
  "last_id": "msg_abc123",
  "has_more": false
}
```
